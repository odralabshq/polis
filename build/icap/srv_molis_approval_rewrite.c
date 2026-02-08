/*
 * srv_molis_approval_rewrite.c - c-ICAP REQMOD approval code rewriter
 *
 * REQMOD service that scans outbound HTTP request bodies for
 * /polis-approve req-* commands and rewrites the request_id with
 * a random OTT (One-Time Token) code. The OTT is stored in Valkey
 * with a time-gate and origin_host for context binding.
 *
 * Security mitigations:
 *   - OTT generation via /dev/urandom only (no PRNG fallback, CWE-330)
 *   - Fail-closed on urandom failure (CWE-457)
 *   - SET ... NX EX for collision-safe OTT storage
 *   - MAX_BODY_SCAN limit to prevent resource exhaustion (CWE-400)
 *   - request_id format validation (CWE-116)
 */

/* c-ICAP headers */
#include "c_icap/c-icap.h"
#include "c_icap/service.h"
#include "c_icap/header.h"
#include "c_icap/body.h"
#include "c_icap/simple_api.h"

/* Standard library headers */
#include <regex.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

/* Valkey/Redis client */
#include <hiredis/hiredis.h>
#include <hiredis/hiredis_ssl.h>

/* Constants */
#define MAX_BODY_SCAN   2097152   /* 2MB body scan limit (CWE-400) */
#define OTT_LEN         12        /* "ott-" + 8 alphanumeric chars */

/* Static configuration — loaded at service init */
static int time_gate_secs = 15;   /* Default time-gate delay */
static int ott_ttl_secs   = 600;  /* OTT key TTL in Valkey */
static regex_t approve_pattern;   /* Compiled regex for /polis-approve */
static redisContext *valkey_ctx = NULL;  /* Valkey connection */

/*
 * rewrite_req_data_t - Per-request state for body accumulation
 * during REQMOD processing of approval commands.
 */
typedef struct {
    ci_membuf_t *body;          /* Accumulated request body */
    size_t total_body_len;      /* Total body length seen so far */
    char host[256];             /* Destination Host header value */
} rewrite_req_data_t;

/* Forward declarations for service callbacks */
int rewrite_init_service(ci_service_xdata_t *srv_xdata,
                         struct ci_server_conf *server_conf);
void rewrite_close_service(void);
void *rewrite_init_request_data(ci_request_t *req);
void rewrite_release_request_data(void *data);
int rewrite_check_preview(char *preview_data, int preview_data_len,
                          ci_request_t *req);
int rewrite_process(ci_request_t *req);
int rewrite_io(char *wbuf, int *wlen, char *rbuf, int *rlen,
               int iseof, ci_request_t *req);

/* Forward declaration for OTT generation */
static int generate_ott(char *buf, size_t buf_len);
static int ensure_valkey_connected(void);

/*
 * Service module definition - exported to c-ICAP.
 * Registers the approval rewriter as a REQMOD service
 * named "molis_approval_rewrite".
 */
CI_DECLARE_MOD_DATA ci_service_module_t service = {
    "molis_approval_rewrite",                    /* mod_name */
    "Molis approval code rewriter (REQMOD)",     /* mod_short_descr */
    ICAP_REQMOD,                                 /* mod_type */
    rewrite_init_service,                        /* mod_init_service */
    NULL,                                        /* mod_post_init_service */
    rewrite_close_service,                       /* mod_close_service */
    rewrite_init_request_data,                   /* mod_init_request_data */
    rewrite_release_request_data,                /* mod_release_request_data */
    rewrite_check_preview,                       /* mod_check_preview_handler */
    rewrite_process,                             /* mod_end_of_data_handler */
    rewrite_io,                                  /* mod_service_io */
    NULL,                                        /* mod_conf_table */
    NULL                                         /* mod_data */
};

/* ------------------------------------------------------------------ */
/* OTT Generation — fail-closed, /dev/urandom only (CWE-330, CWE-457) */
/* ------------------------------------------------------------------ */

/* 62-char alphanumeric alphabet for OTT code characters */
static const char OTT_CHARSET[] =
    "abcdefghijklmnopqrstuvwxyz"
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    "0123456789";
#define OTT_CHARSET_LEN 62  /* sizeof(OTT_CHARSET) - 1 */
#define OTT_RANDOM_BYTES 8  /* Random bytes needed for 8 alphanumeric chars */

/*
 * generate_ott - Generate a One-Time Token using /dev/urandom.
 *
 * Writes "ott-" followed by 8 alphanumeric characters into buf,
 * null-terminated (requires buf_len >= OTT_LEN + 1 = 13).
 *
 * Security: Uses /dev/urandom exclusively. No PRNG fallback.
 * Fail-closed: Returns -1 on any error; caller must abort rewrite.
 *
 * @param buf      Output buffer (must be >= OTT_LEN + 1 bytes)
 * @param buf_len  Size of output buffer
 * @return         0 on success, -1 on failure
 */
static int generate_ott(char *buf, size_t buf_len)
{
    FILE *fp;
    unsigned char random_bytes[OTT_RANDOM_BYTES];
    size_t nread;
    int i;

    /* Validate output buffer can hold "ott-" + 8 chars + '\0' */
    if (buf == NULL || buf_len < (size_t)(OTT_LEN + 1)) {
        ci_debug_printf(0,
            "CRITICAL: generate_ott: buffer too small "
            "(need %d, got %zu)\n",
            OTT_LEN + 1, buf_len);
        return -1;
    }

    /* Open /dev/urandom — fail closed if unavailable */
    fp = fopen("/dev/urandom", "rb");
    if (fp == NULL) {
        ci_debug_printf(0,
            "CRITICAL: generate_ott: cannot open /dev/urandom — "
            "fail closed, no PRNG fallback (CWE-330)\n");
        return -1;
    }

    /* Read 8 random bytes — fail closed on short read */
    nread = fread(random_bytes, 1, OTT_RANDOM_BYTES, fp);
    fclose(fp);

    if (nread != OTT_RANDOM_BYTES) {
        ci_debug_printf(0,
            "CRITICAL: generate_ott: /dev/urandom short read "
            "(%zu of %d bytes) — fail closed (CWE-457)\n",
            nread, OTT_RANDOM_BYTES);
        return -1;
    }

    /* Write "ott-" prefix */
    buf[0] = 'o';
    buf[1] = 't';
    buf[2] = 't';
    buf[3] = '-';

    /* Map each random byte to alphanumeric charset */
    for (i = 0; i < OTT_RANDOM_BYTES; i++) {
        buf[4 + i] = OTT_CHARSET[random_bytes[i] % OTT_CHARSET_LEN];
    }

    /* Null-terminate */
    buf[OTT_LEN] = '\0';

    return 0;
}

/* ------------------------------------------------------------------ */
/* Service Initialization — config loading, regex, Valkey connection   */
/* ------------------------------------------------------------------ */

/*
 * rewrite_init_service - Initialize the approval rewriter service.
 *
 * Performs four setup steps:
 *   1. Load time-gate duration from MOLIS_APPROVAL_TIME_GATE_SECS env
 *      (default: 15 seconds per Requirement 1.10)
 *   2. Compile the approve pattern regex for body scanning
 *   3. Connect to Valkey with TLS + ACL as governance-reqmod
 *   4. Set ICAP preview to 8192 bytes and enable 204 responses
 *
 * Returns CI_OK on success. Valkey connection failure is logged but
 * does not prevent service startup (fail-open for availability;
 * individual requests will fail closed when Valkey is unavailable).
 */
int rewrite_init_service(ci_service_xdata_t *srv_xdata,
                         struct ci_server_conf *server_conf)
{
    const char *env_val;
    int rc;

    ci_debug_printf(3, "molis_approval_rewrite: "
                       "Initializing service\n");

    /* ---------------------------------------------------------- */
    /* Step 1: Load time-gate from environment (Requirement 1.10) */
    /* ---------------------------------------------------------- */
    env_val = getenv("MOLIS_APPROVAL_TIME_GATE_SECS");
    if (env_val != NULL) {
        int parsed = atoi(env_val);
        if (parsed > 0) {
            time_gate_secs = parsed;
            ci_debug_printf(3, "molis_approval_rewrite: "
                "time_gate_secs set to %d from env\n",
                time_gate_secs);
        } else {
            ci_debug_printf(1, "molis_approval_rewrite: WARNING: "
                "invalid MOLIS_APPROVAL_TIME_GATE_SECS='%s', "
                "using default %d\n", env_val, time_gate_secs);
        }
    } else {
        ci_debug_printf(3, "molis_approval_rewrite: "
            "MOLIS_APPROVAL_TIME_GATE_SECS not set, "
            "using default %d\n", time_gate_secs);
    }

    /* ---------------------------------------------------------- */
    /* Step 2: Compile approve pattern regex                      */
    /* ---------------------------------------------------------- */
    rc = regcomp(&approve_pattern,
                 "/polis-approve[[:space:]]+(req-[a-f0-9]{8})",
                 REG_EXTENDED);
    if (rc != 0) {
        char errbuf[128];
        regerror(rc, &approve_pattern, errbuf, sizeof(errbuf));
        ci_debug_printf(0, "molis_approval_rewrite: CRITICAL: "
            "Failed to compile approve pattern regex: %s\n",
            errbuf);
        return CI_ERROR;
    }
    ci_debug_printf(3, "molis_approval_rewrite: "
                       "Approve pattern regex compiled\n");

    /* ---------------------------------------------------------- */
    /* Step 3: Connect to Valkey with TLS + ACL                   */
    /*         User: governance-reqmod (least-privilege)           */
    /*         Env vars:                                          */
    /*           VALKEY_HOST (default: "valkey")                   */
    /*           VALKEY_PORT (default: 6379)                       */
    /*           VALKEY_REQMOD_PASS (required for ACL auth)        */
    /*           VALKEY_TLS_CERT, VALKEY_TLS_KEY, VALKEY_TLS_CA   */
    /* ---------------------------------------------------------- */
    {
        const char *vk_host = getenv("VALKEY_HOST");
        const char *vk_port_str = getenv("VALKEY_PORT");
        const char *vk_pass = getenv("VALKEY_REQMOD_PASS");
        const char *tls_cert = getenv("VALKEY_TLS_CERT");
        const char *tls_key  = getenv("VALKEY_TLS_KEY");
        const char *tls_ca   = getenv("VALKEY_TLS_CA");
        int vk_port;
        redisSSLContext *ssl_ctx = NULL;
        redisSSLContextError ssl_err;
        redisReply *reply;

        if (!vk_host) vk_host = "valkey";
        vk_port = vk_port_str ? atoi(vk_port_str) : 6379;

        /* Initialize OpenSSL for hiredis TLS */
        redisInitOpenSSL();

        /* Create TLS context with client certificates for mTLS */
        ssl_ctx = redisCreateSSLContext(
            tls_ca   ? tls_ca   : "/etc/valkey/tls/ca.crt",
            NULL,  /* capath — not used, single CA file */
            tls_cert ? tls_cert : "/etc/valkey/tls/client.crt",
            tls_key  ? tls_key  : "/etc/valkey/tls/client.key",
            NULL,  /* server_name — use default */
            &ssl_err);
        if (ssl_ctx == NULL) {
            ci_debug_printf(1, "molis_approval_rewrite: WARNING: "
                "Failed to create TLS context: %s — "
                "Valkey connection unavailable\n",
                redisSSLContextGetError(ssl_err));
            goto valkey_done;
        }

        /* Establish TCP connection to Valkey */
        valkey_ctx = redisConnect(vk_host, vk_port);
        if (valkey_ctx == NULL || valkey_ctx->err) {
            ci_debug_printf(1, "molis_approval_rewrite: WARNING: "
                "Cannot connect to Valkey at %s:%d%s%s — "
                "Valkey connection unavailable\n",
                vk_host, vk_port,
                valkey_ctx ? ": " : "",
                valkey_ctx ? valkey_ctx->errstr : "");
            if (valkey_ctx) {
                redisFree(valkey_ctx);
                valkey_ctx = NULL;
            }
            redisFreeSSLContext(ssl_ctx);
            goto valkey_done;
        }

        /* Initiate TLS handshake on the connection */
        if (redisInitiateSSLWithContext(valkey_ctx,
                                        ssl_ctx) != REDIS_OK) {
            ci_debug_printf(1, "molis_approval_rewrite: WARNING: "
                "TLS handshake failed with Valkey: %s — "
                "Valkey connection unavailable\n",
                valkey_ctx->errstr);
            redisFree(valkey_ctx);
            valkey_ctx = NULL;
            redisFreeSSLContext(ssl_ctx);
            goto valkey_done;
        }

        /* Authenticate with ACL: AUTH governance-reqmod <password> */
        if (vk_pass) {
            reply = redisCommand(valkey_ctx,
                "AUTH governance-reqmod %s", vk_pass);
            if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
                ci_debug_printf(1, "molis_approval_rewrite: WARNING: "
                    "Valkey ACL auth failed%s%s — "
                    "Valkey connection unavailable\n",
                    reply ? ": " : "",
                    reply ? reply->str : "");
                if (reply) freeReplyObject(reply);
                redisFree(valkey_ctx);
                valkey_ctx = NULL;
                redisFreeSSLContext(ssl_ctx);
                goto valkey_done;
            }
            freeReplyObject(reply);
            ci_debug_printf(3, "molis_approval_rewrite: "
                "Authenticated as governance-reqmod\n");
        } else {
            ci_debug_printf(1, "molis_approval_rewrite: WARNING: "
                "VALKEY_REQMOD_PASS not set — "
                "ACL authentication skipped\n");
        }

        ci_debug_printf(3, "molis_approval_rewrite: "
            "Connected to Valkey at %s:%d (TLS + ACL)\n",
            vk_host, vk_port);

        redisFreeSSLContext(ssl_ctx);
    }
valkey_done:

    /* ---------------------------------------------------------- */
    /* Step 4: Configure ICAP service parameters                  */
    /* ---------------------------------------------------------- */
    ci_service_set_preview(srv_xdata, 8192);
    ci_service_enable_204(srv_xdata);

    ci_debug_printf(3, "molis_approval_rewrite: "
        "Initialization complete (time_gate=%ds, "
        "ott_ttl=%ds, valkey=%s)\n",
        time_gate_secs, ott_ttl_secs,
        valkey_ctx ? "connected" : "unavailable");

    return CI_OK;
}

/* ------------------------------------------------------------------ */
/* Lazy Valkey reconnection helper (Finding 5 fix)                    */
/* Checks if the Valkey context is still usable and attempts to       */
/* reconnect if the connection was lost (e.g., Valkey restart).       */
/* Returns 1 if connected, 0 if reconnection failed.                 */
/* ------------------------------------------------------------------ */
static int ensure_valkey_connected(void)
{
    redisReply *reply;

    if (valkey_ctx == NULL)
        return 0;

    /* Quick health check with PING */
    reply = redisCommand(valkey_ctx, "PING");
    if (reply != NULL && reply->type != REDIS_REPLY_ERROR) {
        freeReplyObject(reply);
        return 1;
    }
    if (reply) freeReplyObject(reply);

    /* Connection is dead — attempt reconnect */
    ci_debug_printf(1, "molis_approval_rewrite: "
        "Valkey connection lost — attempting reconnect\n");

    if (redisReconnect(valkey_ctx) != REDIS_OK) {
        ci_debug_printf(1, "molis_approval_rewrite: WARNING: "
            "Valkey reconnect failed: %s\n",
            valkey_ctx->errstr);
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
        return 0;
    }

    /* Re-authenticate after reconnect */
    {
        const char *vk_pass = getenv("VALKEY_REQMOD_PASS");
        if (vk_pass) {
            reply = redisCommand(valkey_ctx,
                "AUTH governance-reqmod %s", vk_pass);
            if (reply == NULL ||
                reply->type == REDIS_REPLY_ERROR) {
                ci_debug_printf(1,
                    "molis_approval_rewrite: WARNING: "
                    "Valkey re-auth failed after reconnect\n");
                if (reply) freeReplyObject(reply);
                redisFree(valkey_ctx);
                valkey_ctx = NULL;
                return 0;
            }
            freeReplyObject(reply);
        }
    }

    ci_debug_printf(3, "molis_approval_rewrite: "
        "Valkey reconnected successfully\n");
    return 1;
}

/* ------------------------------------------------------------------ */
/* Request Processing — body scanning and OTT rewriting               */
/* Requirements: 1.2, 1.3, 1.4, 1.6, 1.7, 1.8, 1.9                  */
/* ------------------------------------------------------------------ */

/*
 * rewrite_process - Scan request body for /polis-approve commands
 *                   and rewrite request_id with OTT code.
 *
 * Called by c-ICAP after all body data has been received.
 *
 * Processing steps:
 *   1. Enforce MAX_BODY_SCAN limit (CWE-400)
 *   2. Regex scan for /polis-approve req-{hex8}
 *   3. Validate request_id format (CWE-116)
 *   4. Check molis:blocked:{request_id} exists in Valkey
 *   5. Generate OTT via /dev/urandom (fail-closed)
 *   6. Store OTT mapping with SET ... NX EX (collision-safe)
 *   7. Log rewrite to molis:log:events
 *   8. Perform length-preserving body substitution
 *
 * Returns CI_MOD_ALLOW204 if no rewrite needed, CI_MOD_DONE
 * after successful body modification.
 */
int rewrite_process(ci_request_t *req)
{
    rewrite_req_data_t *data = ci_service_data(req);
    regmatch_t matches[2];
    char *body_raw;
    char req_id[16];       /* "req-" + 8 hex + '\0' = 13 chars */
    char ott_buf[OTT_LEN + 1];  /* "ott-" + 8 alnum + '\0' */
    char valkey_key[64];
    size_t req_id_len;
    int i;

    if (!data || !data->body)
        return CI_MOD_ALLOW204;

    /* -------------------------------------------------------------- */
    /* Step 1: Enforce MAX_BODY_SCAN limit (Req 1.8, CWE-400)        */
    /* -------------------------------------------------------------- */
    if (data->total_body_len > MAX_BODY_SCAN) {
        ci_debug_printf(3, "molis_approval_rewrite: "
            "Body size %zu exceeds MAX_BODY_SCAN (%d) — "
            "skipping scan (CWE-400)\n",
            data->total_body_len, MAX_BODY_SCAN);
        return CI_MOD_ALLOW204;
    }

    /* Null-terminate the body membuf for regex scanning */
    ci_membuf_write(data->body, "\0", 1, 1);
    body_raw = (char *)ci_membuf_raw(data->body);

    if (!body_raw) {
        ci_debug_printf(5, "molis_approval_rewrite: "
            "Empty body buffer — no scan needed\n");
        return CI_MOD_ALLOW204;
    }

    /* -------------------------------------------------------------- */
    /* Step 2: Regex scan for /polis-approve req-{hex8} (Req 1.2)    */
    /* -------------------------------------------------------------- */
    if (regexec(&approve_pattern, body_raw, 2, matches, 0) != 0) {
        ci_debug_printf(5, "molis_approval_rewrite: "
            "No /polis-approve pattern found in body\n");
        return CI_MOD_ALLOW204;
    }

    /* Extract the captured request_id (group 1) */
    req_id_len = (size_t)(matches[1].rm_eo - matches[1].rm_so);
    if (req_id_len == 0 || req_id_len >= sizeof(req_id)) {
        ci_debug_printf(3, "molis_approval_rewrite: "
            "Captured request_id has invalid length %zu\n",
            req_id_len);
        return CI_MOD_ALLOW204;
    }
    memcpy(req_id, body_raw + matches[1].rm_so, req_id_len);
    req_id[req_id_len] = '\0';

    /* -------------------------------------------------------------- */
    /* Step 3: Validate request_id format (Req 1.4, CWE-116)        */
    /* "req-" prefix + exactly 8 lowercase hex chars = 12 chars      */
    /* -------------------------------------------------------------- */
    if (req_id_len != OTT_LEN ||
        strncmp(req_id, "req-", 4) != 0) {
        ci_debug_printf(3, "molis_approval_rewrite: "
            "Invalid request_id format: '%s' (CWE-116)\n",
            req_id);
        return CI_MOD_ALLOW204;
    }
    for (i = 4; i < (int)req_id_len; i++) {
        if (!((req_id[i] >= '0' && req_id[i] <= '9') ||
              (req_id[i] >= 'a' && req_id[i] <= 'f'))) {
            ci_debug_printf(3, "molis_approval_rewrite: "
                "Invalid hex char in request_id: '%s' "
                "(CWE-116)\n", req_id);
            return CI_MOD_ALLOW204;
        }
    }

    ci_debug_printf(3, "molis_approval_rewrite: "
        "Found valid request_id: '%s'\n", req_id);

    /* -------------------------------------------------------------- */
    /* Step 4: Check molis:blocked:{request_id} exists (Req 1.3)     */
    /* -------------------------------------------------------------- */
    if (!valkey_ctx) {
        ci_debug_printf(1, "molis_approval_rewrite: "
            "Valkey unavailable — fail closed, "
            "no OTT rewrite for '%s'\n", req_id);
        return CI_MOD_ALLOW204;
    }

    /* Lazy reconnect if connection was lost (Finding 5 fix) */
    if (!ensure_valkey_connected()) {
        ci_debug_printf(1, "molis_approval_rewrite: "
            "Valkey reconnect failed — fail closed, "
            "no OTT rewrite for '%s'\n", req_id);
        return CI_MOD_ALLOW204;
    }

    {
        redisReply *reply;

        snprintf(valkey_key, sizeof(valkey_key),
                 "molis:blocked:%s", req_id);

        reply = redisCommand(valkey_ctx,
                             "EXISTS %s", valkey_key);
        if (!reply || reply->type == REDIS_REPLY_ERROR) {
            ci_debug_printf(1, "molis_approval_rewrite: "
                "Valkey EXISTS failed for '%s'%s%s\n",
                valkey_key,
                reply ? ": " : "",
                reply ? reply->str : "");
            if (reply) freeReplyObject(reply);
            return CI_MOD_ALLOW204;
        }

        if (reply->integer == 0) {
            ci_debug_printf(3, "molis_approval_rewrite: "
                "No blocked entry for '%s' — "
                "skipping rewrite\n", req_id);
            freeReplyObject(reply);
            return CI_MOD_ALLOW204;
        }
        freeReplyObject(reply);
    }

    ci_debug_printf(3, "molis_approval_rewrite: "
        "Blocked entry found for '%s'\n", req_id);

    /* -------------------------------------------------------------- */
    /* Step 5: Capture destination Host header (Req 1.7)             */
    /* Context binding: OTT is bound to the originating host         */
    /* -------------------------------------------------------------- */
    if (data->host[0] == '\0') {
        ci_debug_printf(1, "molis_approval_rewrite: "
            "No Host header available for context binding — "
            "fail closed, no OTT rewrite\n");
        return CI_MOD_ALLOW204;
    }

    /* -------------------------------------------------------------- */
    /* Step 6a: Generate OTT via /dev/urandom (Req 1.5)              */
    /* Fail-closed: abort rewrite if generation fails                 */
    /* -------------------------------------------------------------- */
    if (generate_ott(ott_buf, sizeof(ott_buf)) != 0) {
        ci_debug_printf(0, "CRITICAL: molis_approval_rewrite: "
            "OTT generation failed — fail closed, "
            "no rewrite for '%s'\n", req_id);
        return CI_MOD_ALLOW204;
    }

    ci_debug_printf(3, "molis_approval_rewrite: "
        "Generated OTT '%s' for '%s'\n", ott_buf, req_id);

    /* -------------------------------------------------------------- */
    /* Step 6b: Store OTT mapping with SET ... NX EX (Req 1.6, 1.7) */
    /* NX = only set if key does not exist (collision-safe)          */
    /* EX = set TTL in seconds                                       */
    /* Retry once on collision with a fresh OTT                      */
    /* -------------------------------------------------------------- */
    {
        redisReply *reply;
        char ott_json[512];
        time_t armed_after;
        int attempt;

        armed_after = time(NULL) + time_gate_secs;

        for (attempt = 0; attempt < 2; attempt++) {
            if (attempt == 1) {
                /* Collision on first attempt — regenerate OTT */
                ci_debug_printf(3, "molis_approval_rewrite: "
                    "OTT collision on '%s', retrying "
                    "with new OTT\n", ott_buf);
                if (generate_ott(ott_buf,
                                 sizeof(ott_buf)) != 0) {
                    ci_debug_printf(0,
                        "CRITICAL: molis_approval_rewrite: "
                        "OTT regeneration failed — "
                        "fail closed\n");
                    return CI_MOD_ALLOW204;
                }
            }

            snprintf(ott_json, sizeof(ott_json),
                "{\"ott_code\":\"%s\","
                "\"request_id\":\"%s\","
                "\"armed_after\":%ld,"
                "\"origin_host\":\"%s\"}",
                ott_buf, req_id,
                (long)armed_after, data->host);

            snprintf(valkey_key, sizeof(valkey_key),
                     "molis:ott:%s", ott_buf);

            reply = redisCommand(valkey_ctx,
                "SET %s %s NX EX %d",
                valkey_key, ott_json, ott_ttl_secs);

            if (!reply ||
                reply->type == REDIS_REPLY_ERROR) {
                ci_debug_printf(1,
                    "molis_approval_rewrite: "
                    "Valkey SET failed for '%s'%s%s\n",
                    valkey_key,
                    reply ? ": " : "",
                    reply ? reply->str : "");
                if (reply) freeReplyObject(reply);
                return CI_MOD_ALLOW204;
            }

            /* SET NX returns nil if key already exists */
            if (reply->type == REDIS_REPLY_NIL) {
                freeReplyObject(reply);
                /* Collision — retry loop continues */
                continue;
            }

            /* Success — key was set */
            freeReplyObject(reply);
            ci_debug_printf(3,
                "molis_approval_rewrite: "
                "Stored OTT mapping '%s' "
                "(ttl=%ds, armed_after=%ld)\n",
                valkey_key, ott_ttl_secs,
                (long)armed_after);
            break;
        }

        /* If we exhausted both attempts, fail closed */
        if (attempt >= 2) {
            ci_debug_printf(0,
                "CRITICAL: molis_approval_rewrite: "
                "OTT collision on both attempts — "
                "fail closed, no rewrite for '%s'\n",
                req_id);
            return CI_MOD_ALLOW204;
        }

        /* ---------------------------------------------------------- */
        /* Step 7: Log rewrite to molis:log:events (Req 1.9)         */
        /* ZADD with timestamp score for ordered event log            */
        /* Log full mapping but never credential values               */
        /* ---------------------------------------------------------- */
        {
            char log_entry[512];
            double now_score = (double)time(NULL);

            snprintf(log_entry, sizeof(log_entry),
                "{\"event\":\"ott_rewrite\","
                "\"ott_code\":\"%s\","
                "\"request_id\":\"%s\","
                "\"origin_host\":\"%s\","
                "\"armed_after\":%ld,"
                "\"timestamp\":%ld}",
                ott_buf, req_id, data->host,
                (long)armed_after, (long)time(NULL));

            reply = redisCommand(valkey_ctx,
                "ZADD molis:log:events %f %s",
                now_score, log_entry);

            if (!reply ||
                reply->type == REDIS_REPLY_ERROR) {
                ci_debug_printf(1,
                    "molis_approval_rewrite: WARNING: "
                    "Failed to log OTT rewrite%s%s "
                    "— continuing with rewrite\n",
                    reply ? ": " : "",
                    reply ? reply->str : "");
            }
            if (reply) freeReplyObject(reply);

            ci_debug_printf(3,
                "molis_approval_rewrite: "
                "Logged OTT rewrite event for '%s'\n",
                req_id);
        }

        /* ---------------------------------------------------------- */
        /* Step 8: Length-preserving body substitution (Req 1.3)     */
        /* Both req_id and OTT are 12 chars — direct memcpy          */
        /* ---------------------------------------------------------- */
        if (req_id_len != OTT_LEN) {
            /* Safety check: lengths must match for in-place swap */
            ci_debug_printf(0,
                "CRITICAL: molis_approval_rewrite: "
                "Length mismatch: req_id=%zu, OTT=%d — "
                "aborting substitution\n",
                req_id_len, OTT_LEN);
            return CI_MOD_ALLOW204;
        }

        /* Overwrite request_id with OTT in the body buffer */
        memcpy(body_raw + matches[1].rm_so,
               ott_buf, OTT_LEN);

        ci_debug_printf(3, "molis_approval_rewrite: "
            "Rewrote '%s' → '%s' in body "
            "(length-preserving, %zu bytes)\n",
            req_id, ott_buf, req_id_len);
    }

    /* Body was modified in-place — return CI_MOD_DONE to tell
     * c-ICAP to forward the request with the modified body.
     * No HTTP response is created; this is REQMOD, not RESPMOD. */
    ci_debug_printf(3, "molis_approval_rewrite: "
        "OTT rewrite complete for '%s' → '%s' "
        "(host=%s)\n", req_id, ott_buf, data->host);

    return CI_MOD_DONE;
}

/* ------------------------------------------------------------------ */
/* Lifecycle Callbacks — per-request data and service teardown         */
/* Requirement: 1.1                                                    */
/* ------------------------------------------------------------------ */

/*
 * rewrite_init_request_data - Allocate and initialize per-request data.
 *
 * Called by c-ICAP for each new REQMOD request. Allocates the
 * rewrite_req_data_t struct, creates a memory buffer for body
 * accumulation, and extracts the Host header from the request
 * for context binding (Requirement 1.7).
 *
 * Returns pointer to the allocated request data, or NULL on failure.
 */
void *rewrite_init_request_data(ci_request_t *req)
{
    rewrite_req_data_t *data;
    const char *host_hdr;

    data = (rewrite_req_data_t *)malloc(sizeof(rewrite_req_data_t));
    if (!data) {
        ci_debug_printf(1, "molis_approval_rewrite: ERROR: "
                           "Failed to allocate request data\n");
        return NULL;
    }

    /* Create memory buffer for body accumulation (up to 2MB) */
    data->body = ci_membuf_new_sized(MAX_BODY_SCAN);
    data->total_body_len = 0;
    data->host[0] = '\0';

    /* Extract Host header from the HTTP request for context binding */
    host_hdr = ci_http_request_get_header(req, "Host");
    if (host_hdr) {
        strncpy(data->host, host_hdr, sizeof(data->host) - 1);
        data->host[sizeof(data->host) - 1] = '\0';
        ci_debug_printf(5, "molis_approval_rewrite: "
                           "Request to host: %s\n", data->host);
    } else {
        ci_debug_printf(5, "molis_approval_rewrite: "
                           "No Host header found\n");
    }

    return data;
}

/*
 * rewrite_release_request_data - Free per-request data.
 *
 * Called by c-ICAP when a request is complete. Frees the body
 * memory buffer and the request data struct.
 */
void rewrite_release_request_data(void *data)
{
    rewrite_req_data_t *req_data = (rewrite_req_data_t *)data;
    if (!req_data)
        return;

    if (req_data->body) {
        ci_membuf_free(req_data->body);
        req_data->body = NULL;
    }

    free(req_data);
}

/*
 * rewrite_check_preview - Handle ICAP preview data.
 *
 * Accumulates the preview chunk into the body memory buffer
 * and updates the total body length counter. Returns
 * CI_MOD_CONTINUE to request the full request body.
 */
int rewrite_check_preview(char *preview_data, int preview_data_len,
                          ci_request_t *req)
{
    rewrite_req_data_t *data = ci_service_data(req);

    if (!data || !preview_data || preview_data_len <= 0)
        return CI_MOD_CONTINUE;

    ci_membuf_write(data->body, preview_data, preview_data_len, 0);
    data->total_body_len += preview_data_len;

    ci_debug_printf(5, "molis_approval_rewrite: "
                       "Preview received %d bytes, "
                       "total so far: %zu\n",
                   preview_data_len, data->total_body_len);

    return CI_MOD_CONTINUE;
}

/*
 * rewrite_io - Handle body data streaming during REQMOD.
 *
 * Accumulates request body data into the ci_membuf up to
 * MAX_BODY_SCAN (2MB). Unlike the DLP module, the rewriter
 * does not need a tail buffer — bodies exceeding 2MB are
 * simply skipped at scan time (CWE-400).
 *
 * We never modify the request body during streaming (wlen = 0);
 * modification happens in rewrite_process() after full accumulation.
 *
 * Returns CI_OK on success.
 */
int rewrite_io(char *wbuf, int *wlen, char *rbuf, int *rlen,
               int iseof, ci_request_t *req)
{
    rewrite_req_data_t *data = ci_service_data(req);
    int bytes_to_read;
    int membuf_space;
    int membuf_write;

    (void)iseof;

    /* We don't modify the request body during streaming */
    if (wbuf && wlen)
        *wlen = 0;

    if (!data || !rbuf || !rlen || *rlen <= 0)
        return CI_OK;

    bytes_to_read = *rlen;

    /* Accumulate into membuf up to MAX_BODY_SCAN */
    if (data->total_body_len < MAX_BODY_SCAN) {
        membuf_space = MAX_BODY_SCAN - (int)data->total_body_len;
        membuf_write = (bytes_to_read < membuf_space)
                       ? bytes_to_read : membuf_space;
        ci_membuf_write(data->body, rbuf, membuf_write, 0);
    }

    /* Always track total body length for the size check */
    data->total_body_len += bytes_to_read;

    return CI_OK;
}

/*
 * rewrite_close_service - Clean up when the rewriter service shuts down.
 *
 * Frees the compiled approve pattern regex and disconnects from
 * Valkey to avoid resource leaks.
 */
void rewrite_close_service(void)
{
    ci_debug_printf(3, "molis_approval_rewrite: "
                       "Closing service\n");

    /* Free the compiled regex pattern */
    regfree(&approve_pattern);

    /* Disconnect from Valkey if connected */
    if (valkey_ctx) {
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
    }

    ci_debug_printf(3, "molis_approval_rewrite: "
                       "Service closed, resources freed\n");
}
