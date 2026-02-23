/*
 * srv_polis_approval.c - c-ICAP RESPMOD OTT scanner for approval detection
 *
 * RESPMOD service that scans inbound HTTP response bodies from
 * allowlisted messaging domains for OTT (One-Time Token) codes.
 * When a valid OTT is found, the module resolves it to a request_id,
 * validates the time-gate and context binding, preserves audit data,
 * writes the approval to Valkey, and strips the OTT from the response.
 *
 * Security mitigations:
 *   - Dot-prefixed domain allowlist with dot-boundary matching (CWE-346)
 *   - Time-gated OTT arming to prevent sendMessage echo self-approval
 *   - Context binding: OTT origin_host must match response host
 *   - MAX_BODY_SCAN limit to prevent resource exhaustion (CWE-400)
 *   - Audit trail preservation before blocked key deletion
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
#include <zlib.h>

/* Valkey/Redis client */
#include <hiredis/hiredis.h>
#include <hiredis/hiredis_ssl.h>

/* Constants */
#define MAX_BODY_SCAN       2097152   /* 2MB body scan limit (CWE-400) */
#define APPROVAL_TTL_SECS   300       /* Approval key TTL: 5 minutes */
#define MAX_DOMAINS         16        /* Maximum entries in domain allowlist */
#define OTT_LEN             12        /* "ott-" + 8 alphanumeric chars */

/*
 * Static domain allowlist — dot-prefixed for dot-boundary matching.
 * Loaded from polis_APPROVAL_DOMAINS env var or defaults at init.
 * Dot-prefix ensures ".slack.com" matches "api.slack.com" but NOT
 * "evil-slack.com" (CWE-346 prevention).
 */
static char allowed_domains[MAX_DOMAINS][256];
static int  domain_count = 0;

/* Default dot-prefixed domains (used when env var is not set) */
static const char *DEFAULT_DOMAINS[] = {
    ".api.telegram.org",
    ".api.slack.com",
    ".discord.com",
    NULL
};

/* Compiled OTT regex pattern: ott-[a-zA-Z0-9]{8} */
static regex_t ott_pattern;

/* Valkey connection for OTT lookup and approval writes */
static redisContext *valkey_ctx = NULL;

/*
 * approval_req_data_t - Per-request state for body accumulation
 * during RESPMOD processing of approval responses.
 */
typedef struct {
    ci_membuf_t *body;          /* Accumulated response body */
    size_t total_body_len;      /* Total body length seen so far */
    char host[256];             /* Response Host header value */
    int is_gzip;                /* 1 if Content-Encoding is gzip */
} approval_req_data_t;

/* Forward declarations for service callbacks */
int approval_init_service(ci_service_xdata_t *srv_xdata,
                          struct ci_server_conf *server_conf);
void approval_close_service(void);
void *approval_init_request_data(ci_request_t *req);
void approval_release_request_data(void *data);
int approval_check_preview(char *preview_data, int preview_data_len,
                           ci_request_t *req);
int approval_process(ci_request_t *req);
int approval_io(char *wbuf, int *wlen, char *rbuf, int *rlen,
                int iseof, ci_request_t *req);

/* Forward declarations for internal functions */
static int is_allowed_domain(const char *host);
static int process_ott_approval(const char *ott_code,
                                const char *resp_host);
static int ensure_valkey_connected(void);

/*
 * Service module definition - exported to c-ICAP.
 * Registers the approval scanner as a RESPMOD service
 * named "polis_approval".
 */
CI_DECLARE_MOD_DATA ci_service_module_t service = {
    "polis_approval",                            /* mod_name */
    "polis approval OTT scanner (RESPMOD)",      /* mod_short_descr */
    ICAP_RESPMOD,                                /* mod_type */
    approval_init_service,                       /* mod_init_service */
    NULL,                                        /* mod_post_init_service */
    approval_close_service,                      /* mod_close_service */
    approval_init_request_data,                  /* mod_init_request_data */
    approval_release_request_data,               /* mod_release_request_data */
    approval_check_preview,                      /* mod_check_preview_handler */
    approval_process,                            /* mod_end_of_data_handler */
    approval_io,                                 /* mod_service_io */
    NULL,                                        /* mod_conf_table */
    NULL                                         /* mod_data */
};

/*
 * is_allowed_domain() — Dot-boundary domain matching (CWE-346)
 *
 * Checks whether the given host matches any entry in the domain allowlist.
 *
 * For dot-prefixed entries (e.g., ".slack.com"):
 *   - Suffix match with implicit dot boundary:
 *     "api.slack.com" matches ".slack.com" because the suffix aligns
 *     at a dot boundary in the host.
 *   - "evil-slack.com" does NOT match ".slack.com" because there is
 *     no dot before "slack.com" in the host — the preceding char is '-'.
 *   - Exact domain without leading dot also matches:
 *     "slack.com" matches ".slack.com" (the entry minus its leading dot).
 *
 * For non-dot-prefixed entries:
 *   - Exact (case-insensitive) match only.
 *
 * Returns: 1 if host is allowed, 0 otherwise.
 *
 * Validates: Requirements 2.2, 2.3
 */
static int is_allowed_domain(const char *host)
{
    int i;
    size_t host_len;
    size_t entry_len;

    if (host == NULL || host[0] == '\0')
        return 0;

    host_len = strlen(host);

    for (i = 0; i < domain_count; i++) {
        const char *entry = allowed_domains[i];
        entry_len = strlen(entry);

        if (entry_len == 0)
            continue;

        if (entry[0] == '.') {
            /*
             * Dot-prefixed entry: two matching modes.
             *
             * Mode 1: Exact match against the domain without
             * the leading dot.
             *   e.g., host "slack.com" matches entry ".slack.com"
             *   Compare host against entry+1 (skip the dot).
             */
            const char *bare = entry + 1;  /* entry without leading dot */
            size_t bare_len = entry_len - 1;

            if (host_len == bare_len &&
                strcasecmp(host, bare) == 0) {
                return 1;
            }

            /*
             * Mode 2: Suffix match with dot-boundary enforcement.
             *   The host must end with the full dot-prefixed entry,
             *   which inherently ensures a dot boundary because the
             *   entry itself starts with '.'.
             *
             *   e.g., host "api.slack.com" (len=13)
             *         entry ".slack.com"   (len=10)
             *         suffix starts at host[3] = ".slack.com" ✓
             *
             *   e.g., host "evil-slack.com" (len=14)
             *         entry ".slack.com"    (len=10)
             *         suffix starts at host[4] = "-slack.com" ✗
             *
             *   Host must be longer than the entry for a suffix
             *   match (otherwise it would be a bare-domain match
             *   handled above, or too short).
             */
            if (host_len > entry_len) {
                const char *suffix = host + (host_len - entry_len);
                if (strcasecmp(suffix, entry) == 0) {
                    return 1;
                }
            }
        } else {
            /*
             * Non-dot-prefixed entry: exact match only.
             * Case-insensitive comparison per DNS conventions.
             */
            if (strcasecmp(host, entry) == 0) {
                return 1;
            }
        }
    }

    return 0;
}

/* ------------------------------------------------------------------ */
/* Service Initialization — config and domain loading                 */
/* Requirements: 2.4, 2.5                                             */
/* ------------------------------------------------------------------ */

/*
 * approval_init_service() — Initialize the RESPMOD approval scanner.
 *
 * Performs three initialization steps:
 *   1. Compile OTT regex pattern: ott-[a-zA-Z0-9]{8}
 *   2. Load domain allowlist from polis_APPROVAL_DOMAINS env var
 *      or fall back to DEFAULT_DOMAINS (dot-prefixed)
 *   3. Connect to Valkey with TLS + ACL as governance-respmod
 *
 * Returns CI_OK on success, CI_ERROR on fatal failure (regex).
 * Valkey connection failure is non-fatal (logged as WARNING).
 */
int approval_init_service(ci_service_xdata_t *srv_xdata,
                          struct ci_server_conf *server_conf)
{
    int rc;

    ci_debug_printf(3, "polis_approval: "
                       "Initializing service\n");

    /* ---------------------------------------------------------- */
    /* Step 1: Compile OTT regex pattern                          */
    /*         Pattern: ott-[a-zA-Z0-9]{8}                        */
    /* ---------------------------------------------------------- */
    rc = regcomp(&ott_pattern,
                 "ott-[a-zA-Z0-9]{8}",
                 REG_EXTENDED);
    if (rc != 0) {
        char errbuf[128];
        regerror(rc, &ott_pattern, errbuf, sizeof(errbuf));
        ci_debug_printf(0, "polis_approval: CRITICAL: "
            "Failed to compile OTT regex: %s\n", errbuf);
        return CI_ERROR;
    }
    ci_debug_printf(3, "polis_approval: "
                       "OTT regex compiled\n");

    /* ---------------------------------------------------------- */
    /* Step 2: Load domain allowlist (Requirements 2.4, 2.5)      */
    /*         Source: polis_APPROVAL_DOMAINS env var              */
    /*         Fallback: DEFAULT_DOMAINS[] (dot-prefixed)          */
    /* ---------------------------------------------------------- */
    {
        const char *env_domains = getenv("polis_APPROVAL_DOMAINS");

        domain_count = 0;

        if (env_domains != NULL && env_domains[0] != '\0') {
            /*
             * Parse comma-separated domains from env var.
             * Each domain is trimmed and stored in allowed_domains[].
             * Example: ".api.telegram.org,.api.slack.com,.discord.com"
             */
            char buf[4096];
            char *token;
            char *saveptr;

            snprintf(buf, sizeof(buf), "%s", env_domains);

            token = strtok_r(buf, ",", &saveptr);
            while (token != NULL && domain_count < MAX_DOMAINS) {
                /* Skip leading whitespace */
                while (*token == ' ' || *token == '\t')
                    token++;

                /* Remove trailing whitespace */
                size_t tlen = strlen(token);
                while (tlen > 0 &&
                       (token[tlen - 1] == ' ' ||
                        token[tlen - 1] == '\t')) {
                    token[--tlen] = '\0';
                }

                if (tlen > 0 && tlen < sizeof(allowed_domains[0])) {
                    snprintf(allowed_domains[domain_count],
                             sizeof(allowed_domains[0]),
                             "%s", token);
                    ci_debug_printf(3, "polis_approval: "
                        "Loaded domain [%d]: %s (from env)\n",
                        domain_count,
                        allowed_domains[domain_count]);
                    domain_count++;
                }

                token = strtok_r(NULL, ",", &saveptr);
            }

            ci_debug_printf(3, "polis_approval: "
                "Loaded %d domain(s) from "
                "polis_APPROVAL_DOMAINS env\n", domain_count);
        } else {
            /*
             * No env var set — use default dot-prefixed domains.
             * Defaults: .api.telegram.org, .api.slack.com,
             *           .discord.com (Requirement 2.4)
             */
            int i;
            for (i = 0; DEFAULT_DOMAINS[i] != NULL &&
                         domain_count < MAX_DOMAINS; i++) {
                snprintf(allowed_domains[domain_count],
                         sizeof(allowed_domains[0]),
                         "%s", DEFAULT_DOMAINS[i]);
                ci_debug_printf(3, "polis_approval: "
                    "Loaded domain [%d]: %s (default)\n",
                    domain_count,
                    allowed_domains[domain_count]);
                domain_count++;
            }

            ci_debug_printf(3, "polis_approval: "
                "Loaded %d default domain(s)\n", domain_count);
        }
    }

    /* ---------------------------------------------------------- */
    /* Step 3: Connect to Valkey with TLS + ACL                   */
    /*         User: governance-respmod (least-privilege)          */
    /*         Password read from /run/secrets/valkey_respmod_password */
    /* ---------------------------------------------------------- */
    {
        const char *vk_host = getenv("VALKEY_HOST");
        const char *vk_port_str = getenv("VALKEY_PORT");
        const char *tls_cert = getenv("VALKEY_TLS_CERT");
        const char *tls_key  = getenv("VALKEY_TLS_KEY");
        const char *tls_ca   = getenv("VALKEY_TLS_CA");
        int vk_port;
        redisSSLContext *ssl_ctx = NULL;
        redisSSLContextError ssl_err;
        redisReply *reply;
        char vk_pass[256];
        FILE *pass_file;

        if (!vk_host) vk_host = "state";
        vk_port = vk_port_str ? atoi(vk_port_str) : 6379;

        /* Read password from Docker secret */
        pass_file = fopen("/run/secrets/valkey_respmod_password", "r");
        if (!pass_file) {
            ci_debug_printf(1, "polis_approval: WARNING: "
                "Cannot open /run/secrets/valkey_respmod_password — "
                "Valkey connection unavailable\n");
            goto valkey_done;
        }
        if (!fgets(vk_pass, sizeof(vk_pass), pass_file)) {
            ci_debug_printf(1, "polis_approval: WARNING: "
                "Cannot read /run/secrets/valkey_respmod_password\n");
            fclose(pass_file);
            goto valkey_done;
        }
        fclose(pass_file);
        vk_pass[strcspn(vk_pass, "\r\n")] = '\0';  /* Strip newline */

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
            ci_debug_printf(1, "polis_approval: WARNING: "
                "Failed to create TLS context: %s — "
                "Valkey connection unavailable\n",
                redisSSLContextGetError(ssl_err));
            goto valkey_done;
        }

        /* Establish TCP connection to Valkey */
        valkey_ctx = redisConnect(vk_host, vk_port);
        if (valkey_ctx == NULL || valkey_ctx->err) {
            ci_debug_printf(1, "polis_approval: WARNING: "
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
            ci_debug_printf(1, "polis_approval: WARNING: "
                "TLS handshake failed with Valkey: %s — "
                "Valkey connection unavailable\n",
                valkey_ctx->errstr);
            redisFree(valkey_ctx);
            valkey_ctx = NULL;
            redisFreeSSLContext(ssl_ctx);
            goto valkey_done;
        }

        /* Authenticate with ACL: AUTH governance-respmod <password> */
        reply = redisCommand(valkey_ctx,
            "AUTH governance-respmod %s", vk_pass);
        if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
            ci_debug_printf(1, "polis_approval: WARNING: "
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
        ci_debug_printf(3, "polis_approval: "
            "Authenticated as governance-respmod\n");

        ci_debug_printf(3, "polis_approval: "
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

    ci_debug_printf(3, "polis_approval: "
        "Initialization complete (domains=%d, valkey=%s)\n",
        domain_count,
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
    ci_debug_printf(1, "polis_approval: "
        "Valkey connection lost — attempting reconnect\n");

    if (redisReconnect(valkey_ctx) != REDIS_OK) {
        ci_debug_printf(1, "polis_approval: WARNING: "
            "Valkey reconnect failed: %s\n",
            valkey_ctx->errstr);
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
        return 0;
    }

    /* Re-authenticate after reconnect */
    {
        char vk_pass[256];
        FILE *pass_file = fopen("/run/secrets/valkey_respmod_password", "r");
        if (pass_file && fgets(vk_pass, sizeof(vk_pass), pass_file)) {
            fclose(pass_file);
            vk_pass[strcspn(vk_pass, "\r\n")] = '\0';

            reply = redisCommand(valkey_ctx,
                "AUTH governance-respmod %s", vk_pass);
            if (reply == NULL ||
                reply->type == REDIS_REPLY_ERROR) {
                ci_debug_printf(1, "polis_approval: WARNING: "
                    "Valkey re-auth failed after reconnect\n");
                if (reply) freeReplyObject(reply);
                redisFree(valkey_ctx);
                valkey_ctx = NULL;
                return 0;
            }
            freeReplyObject(reply);
        } else {
            ci_debug_printf(1, "polis_approval: WARNING: "
                "Cannot read /run/secrets/valkey_respmod_password "
                "during reconnect\n");
            if (pass_file) fclose(pass_file);
            redisFree(valkey_ctx);
            valkey_ctx = NULL;
            return 0;
        }
    }

    ci_debug_printf(3, "polis_approval: "
        "Valkey reconnected successfully\n");
    return 1;
}

/* ------------------------------------------------------------------ */
/* process_ott_approval() — Context-bound approval with audit         */
/* Requirements: 2.6, 2.7, 2.8, 2.9                                  */
/* ------------------------------------------------------------------ */

/*
 * process_ott_approval() — Resolve OTT to request_id and write approval.
 *
 * Performs the full approval flow:
 *   1. GET polis:ott:{ott} → parse JSON mapping
 *   2. Check time-gate: now >= armed_after (Req 2.7)
 *   3. Check context binding: resp_host == origin_host (Req 2.8)
 *   4. Check blocked request exists
 *   5. GET blocked data for audit preservation (Req 2.9)
 *   6. DEL blocked key, SETEX approved key with 5-min TTL
 *   7. ZADD audit log with blocked_request data
 *   8. DEL OTT key
 *
 * Returns: 1 on successful approval, 0 on rejection/skip, -1 on error.
 *
 * Validates: Requirements 2.6, 2.7, 2.8, 2.9
 */
static int process_ott_approval(const char *ott_code,
                                const char *resp_host)
{
    redisReply *reply = NULL;
    char ott_key[64];
    char blocked_key[64];
    char approved_key[64];
    char *ott_json = NULL;
    char *blocked_data = NULL;

    /* Parsed fields from OTT mapping JSON */
    char parsed_request_id[32];
    char parsed_origin_host[256];
    long parsed_armed_after = 0;

    time_t now;

    if (ott_code == NULL || resp_host == NULL) {
        ci_debug_printf(1, "polis_approval: "
            "process_ott_approval: NULL parameter\n");
        return -1;
    }

    if (valkey_ctx == NULL) {
        ci_debug_printf(1, "polis_approval: "
            "process_ott_approval: Valkey unavailable — "
            "cannot process OTT '%s'\n", ott_code);
        return -1;
    }

    /* Lazy reconnect if connection was lost (Finding 5 fix) */
    if (!ensure_valkey_connected()) {
        ci_debug_printf(1, "polis_approval: "
            "process_ott_approval: Valkey reconnect failed — "
            "cannot process OTT '%s'\n", ott_code);
        return -1;
    }

    /* ---------------------------------------------------------- */
    /* Step 1: GET polis:ott:{ott} → parse JSON mapping           */
    /* ---------------------------------------------------------- */
    snprintf(ott_key, sizeof(ott_key),
             "polis:ott:%s", ott_code);

    reply = redisCommand(valkey_ctx, "GET %s", ott_key);
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "polis_approval: "
            "Valkey GET failed for OTT '%s'%s%s\n",
            ott_code,
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        return -1;
    }

    if (reply->type == REDIS_REPLY_NIL || reply->str == NULL) {
        ci_debug_printf(3, "polis_approval: "
            "OTT '%s' not found in Valkey — "
            "expired or invalid\n", ott_code);
        freeReplyObject(reply);
        return 0;
    }

    /* Duplicate the JSON string before freeing reply */
    ott_json = strdup(reply->str);
    freeReplyObject(reply);
    reply = NULL;

    if (ott_json == NULL) {
        ci_debug_printf(0, "polis_approval: CRITICAL: "
            "strdup failed for OTT JSON\n");
        return -1;
    }

    /*
     * Parse OTT mapping JSON — minimal parser for known format:
     * {"ott_code":"...","request_id":"...","armed_after":N,
     *  "origin_host":"..."}
     *
     * We extract request_id, armed_after, and origin_host.
     */
    {
        const char *p;
        const char *end;
        size_t len;

        /* Extract request_id */
        p = strstr(ott_json, "\"request_id\":\"");
        if (p == NULL) {
            ci_debug_printf(1, "polis_approval: "
                "Malformed OTT JSON — missing request_id "
                "for OTT '%s'\n", ott_code);
            free(ott_json);
            return -1;
        }
        p += strlen("\"request_id\":\"");
        end = strchr(p, '"');
        if (end == NULL || (size_t)(end - p) >=
                sizeof(parsed_request_id)) {
            ci_debug_printf(1, "polis_approval: "
                "Malformed request_id in OTT JSON "
                "for OTT '%s'\n", ott_code);
            free(ott_json);
            return -1;
        }
        len = (size_t)(end - p);
        memcpy(parsed_request_id, p, len);
        parsed_request_id[len] = '\0';

        /* Extract armed_after (integer) */
        p = strstr(ott_json, "\"armed_after\":");
        if (p == NULL) {
            ci_debug_printf(1, "polis_approval: "
                "Malformed OTT JSON — missing armed_after "
                "for OTT '%s'\n", ott_code);
            free(ott_json);
            return -1;
        }
        p += strlen("\"armed_after\":");
        parsed_armed_after = strtol(p, NULL, 10);

        /* Extract origin_host */
        p = strstr(ott_json, "\"origin_host\":\"");
        if (p == NULL) {
            ci_debug_printf(1, "polis_approval: "
                "Malformed OTT JSON — missing origin_host "
                "for OTT '%s'\n", ott_code);
            free(ott_json);
            return -1;
        }
        p += strlen("\"origin_host\":\"");
        end = strchr(p, '"');
        if (end == NULL || (size_t)(end - p) >=
                sizeof(parsed_origin_host)) {
            ci_debug_printf(1, "polis_approval: "
                "Malformed origin_host in OTT JSON "
                "for OTT '%s'\n", ott_code);
            free(ott_json);
            return -1;
        }
        len = (size_t)(end - p);
        memcpy(parsed_origin_host, p, len);
        parsed_origin_host[len] = '\0';
    }

    /* OTT JSON no longer needed after parsing */
    free(ott_json);
    ott_json = NULL;

    ci_debug_printf(3, "polis_approval: "
        "OTT '%s' → request_id='%s', origin_host='%s', "
        "armed_after=%ld\n",
        ott_code, parsed_request_id,
        parsed_origin_host, parsed_armed_after);

    /* ---------------------------------------------------------- */
    /* Step 2: Check time-gate — now >= armed_after (Req 2.7)     */
    /* If time-gate has NOT elapsed, ignore the OTT.              */
    /* This prevents self-approval via sendMessage echo.          */
    /* ---------------------------------------------------------- */
    now = time(NULL);
    if ((long)now < parsed_armed_after) {
        ci_debug_printf(3, "polis_approval: "
            "OTT '%s' time-gate not elapsed — "
            "now=%ld < armed_after=%ld — "
            "ignoring (echo protection)\n",
            ott_code, (long)now, parsed_armed_after);
        return 0;
    }

    /* ---------------------------------------------------------- */
    /* Step 3: Check context binding (Req 2.8)                    */
    /* resp_host must match origin_host from OTT mapping.         */
    /* Prevents cross-channel OTT replay attacks.                 */
    /* ---------------------------------------------------------- */
    if (strcasecmp(resp_host, parsed_origin_host) != 0) {
        ci_debug_printf(1, "polis_approval: "
            "OTT '%s' context binding FAILED — "
            "resp_host='%s' != origin_host='%s' — "
            "rejecting (cross-channel replay prevention)\n",
            ott_code, resp_host, parsed_origin_host);
        return 0;
    }

    ci_debug_printf(3, "polis_approval: "
        "OTT '%s' passed time-gate and context binding\n",
        ott_code);

    /* ---------------------------------------------------------- */
    /* Step 4: Check blocked request exists                       */
    /* ---------------------------------------------------------- */
    snprintf(blocked_key, sizeof(blocked_key),
             "polis:blocked:%s", parsed_request_id);

    reply = redisCommand(valkey_ctx,
                         "EXISTS %s", blocked_key);
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "polis_approval: "
            "Valkey EXISTS failed for '%s'%s%s\n",
            blocked_key,
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        return -1;
    }

    if (reply->integer == 0) {
        ci_debug_printf(3, "polis_approval: "
            "Blocked request '%s' not found — "
            "OTT '%s' stale or already processed\n",
            parsed_request_id, ott_code);
        freeReplyObject(reply);
        return 0;
    }
    freeReplyObject(reply);
    reply = NULL;

    /* ---------------------------------------------------------- */
    /* Step 5: GET blocked request data for audit preservation    */
    /* Requirement 2.9: Preserve blocked data BEFORE deletion     */
    /* ---------------------------------------------------------- */
    reply = redisCommand(valkey_ctx, "GET %s", blocked_key);
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "polis_approval: "
            "Valkey GET failed for '%s'%s%s\n",
            blocked_key,
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        return -1;
    }

    if (reply->type != REDIS_REPLY_NIL && reply->str != NULL) {
        blocked_data = strdup(reply->str);
    }
    freeReplyObject(reply);
    reply = NULL;

    if (blocked_data == NULL) {
        ci_debug_printf(1, "polis_approval: "
            "Blocked data for '%s' is empty or "
            "strdup failed — proceeding without "
            "audit data\n", parsed_request_id);
        /* Non-fatal: proceed with approval but log warning */
        blocked_data = strdup("{}");
        if (blocked_data == NULL) {
            ci_debug_printf(0, "polis_approval: CRITICAL: "
                "strdup failed for fallback blocked_data\n");
            return -1;
        }
    }

    ci_debug_printf(3, "polis_approval: "
        "Preserved blocked data for '%s' "
        "(audit trail)\n", parsed_request_id);

    snprintf(approved_key, sizeof(approved_key),
             "polis:approved:%s", parsed_request_id);

    /* ---------------------------------------------------------- */
    /* Step 6: ZADD audit log BEFORE destructive ops (Req 2.9)    */
    /* Audit data must be persisted before the blocked key is      */
    /* deleted, so a crash between steps cannot lose audit data.   */
    /* ---------------------------------------------------------- */
    {
        char *log_entry = NULL;
        size_t log_size;
        double now_score = (double)now;
        const char *bd_fmt;

        /*
         * Validate blocked_data looks like JSON before embedding
         * as a raw value. If it doesn't start with '{', wrap it
         * as a quoted string to prevent audit log corruption
         * (CWE-74 defense-in-depth).
         */
        if (blocked_data[0] == '{') {
            bd_fmt = "\"blocked_request\":%s}";
        } else {
            bd_fmt = "\"blocked_request\":\"%s\"}";
            ci_debug_printf(1, "polis_approval: WARNING: "
                "blocked_data is not JSON object — "
                "embedding as string\n");
        }

        /*
         * Build audit log entry JSON. We allocate a
         * buffer large enough for the template + blocked_data.
         */
        log_size = 512 + strlen(blocked_data);
        log_entry = malloc(log_size);
        if (log_entry == NULL) {
            ci_debug_printf(0, "polis_approval: CRITICAL: "
                "malloc failed for audit log entry\n");
            free(blocked_data);
            return -1;
        }

        snprintf(log_entry, log_size,
            "{\"event\":\"approved_via_proxy\","
            "\"request_id\":\"%s\","
            "\"ott_code\":\"%s\","
            "\"origin_host\":\"%s\","
            "\"timestamp\":%ld,"
            "%s",
            parsed_request_id, ott_code,
            parsed_origin_host, (long)now,
            "");

        /* Append the blocked_request field using the
         * validated format (raw JSON or quoted string) */
        {
            size_t prefix_len = strlen(log_entry);
            snprintf(log_entry + prefix_len,
                     log_size - prefix_len,
                     bd_fmt, blocked_data);
        }

        reply = redisCommand(valkey_ctx,
            "ZADD polis:log:events %f %s",
            now_score, log_entry);

        if (reply == NULL ||
            reply->type == REDIS_REPLY_ERROR) {
            ci_debug_printf(1, "polis_approval: WARNING: "
                "Failed to write audit log%s%s — "
                "aborting approval to preserve data "
                "integrity\n",
                reply ? ": " : "",
                reply ? reply->str : "");
            if (reply) freeReplyObject(reply);
            free(log_entry);
            free(blocked_data);
            return -1;
        }
        ci_debug_printf(3, "polis_approval: "
            "Audit log written for '%s'\n",
            parsed_request_id);
        if (reply) freeReplyObject(reply);
        reply = NULL;

        free(log_entry);
    }

    /* blocked_data no longer needed after audit log write */
    free(blocked_data);
    blocked_data = NULL;

    /* ---------------------------------------------------------- */
    /* Step 7: DEL blocked key, SETEX approved key (Req 2.6)      */
    /* Now safe to destroy source data — audit is persisted.      */
    /* Approval key has 5-minute TTL (APPROVAL_TTL_SECS = 300)    */
    /* ---------------------------------------------------------- */

    /* DEL the blocked key */
    reply = redisCommand(valkey_ctx,
                         "DEL %s", blocked_key);
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "polis_approval: "
            "Valkey DEL failed for '%s'%s%s\n",
            blocked_key,
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        return -1;
    }
    freeReplyObject(reply);
    reply = NULL;

    /* SETEX the approved key with 5-minute TTL */
    reply = redisCommand(valkey_ctx,
        "SETEX %s %d approved",
        approved_key, APPROVAL_TTL_SECS);
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "polis_approval: "
            "Valkey SETEX failed for '%s'%s%s\n",
            approved_key,
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        return -1;
    }
    freeReplyObject(reply);
    reply = NULL;

    ci_debug_printf(3, "polis_approval: "
        "Approved '%s' — SETEX with %ds TTL\n",
        parsed_request_id, APPROVAL_TTL_SECS);

    /* ---------------------------------------------------------- */
    /* Step 8: DEL OTT key — consume the one-time token           */
    /* Done last so that if earlier steps fail, the OTT remains   */
    /* available for retry.                                       */
    /* ---------------------------------------------------------- */
    reply = redisCommand(valkey_ctx, "DEL %s", ott_key);
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "polis_approval: WARNING: "
            "Failed to DEL OTT key '%s'%s%s — "
            "approval still valid, OTT will expire\n",
            ott_key,
            reply ? ": " : "",
            reply ? reply->str : "");
    } else {
        ci_debug_printf(3, "polis_approval: "
            "Deleted OTT key '%s'\n", ott_key);
    }
    if (reply) freeReplyObject(reply);

    ci_debug_printf(3, "polis_approval: "
        "OTT '%s' → request_id '%s' approved "
        "via proxy (origin: %s)\n",
        ott_code, parsed_request_id,
        parsed_origin_host);

    return 1;
}

/* ------------------------------------------------------------------ */
/* approval_process() — RESPMOD body scanning for OTT codes           */
/* Requirements: 2.10, 2.11, 2.12                                    */
/* ------------------------------------------------------------------ */

/*
 * approval_process() — End-of-data handler for RESPMOD approval scan.
 *
 * Called by c-ICAP when the full response body has been accumulated.
 * Performs the following steps:
 *   1. Check Host against domain allowlist (channel scoping)
 *   2. Enforce MAX_BODY_SCAN limit (CWE-400)
 *   3. Handle gzip Content-Encoding (decompress before scan)
 *   4. Scan body for OTT regex pattern
 *   5. Call process_ott_approval() for each OTT found
 *   6. Strip OTT from response body on successful approval
 *   7. Recompress if originally gzip-encoded
 *
 * Returns: CI_MOD_DONE if body was modified, CI_MOD_ALLOW204 otherwise.
 *
 * Validates: Requirements 2.10, 2.11, 2.12
 */
int approval_process(ci_request_t *req)
{
    approval_req_data_t *data;
    const char *body_ptr;
    size_t body_len;
    unsigned char *decompressed = NULL;
    unsigned long decomp_len = 0;
    unsigned char *scan_buf = NULL;
    size_t scan_len = 0;
    int body_modified = 0;
    regmatch_t match;
    size_t search_offset;

    /* ---------------------------------------------------------- */
    /* Step 0: Retrieve per-request data                          */
    /* ---------------------------------------------------------- */
    data = ci_service_data(req);
    if (data == NULL) {
        ci_debug_printf(1, "polis_approval: "
            "approval_process: no request data\n");
        return CI_MOD_ALLOW204;
    }

    /* ---------------------------------------------------------- */
    /* Step 1: Check Host against domain allowlist                */
    /* Non-allowlisted domains are ignored entirely (Req 2.2)     */
    /* ---------------------------------------------------------- */
    if (data->host[0] == '\0') {
        ci_debug_printf(3, "polis_approval: "
            "approval_process: no Host header — "
            "skipping scan\n");
        return CI_MOD_ALLOW204;
    }

    if (!is_allowed_domain(data->host)) {
        ci_debug_printf(5, "polis_approval: "
            "Host '%s' not in domain allowlist — "
            "skipping scan\n", data->host);
        return CI_MOD_ALLOW204;
    }

    ci_debug_printf(3, "polis_approval: "
        "Host '%s' is allowlisted — "
        "scanning body for OTT\n", data->host);

    /* ---------------------------------------------------------- */
    /* Step 2: Enforce MAX_BODY_SCAN limit (Req 2.11, CWE-400)   */
    /* Bodies exceeding 2MB are passed through without scanning   */
    /* to prevent resource exhaustion.                            */
    /* ---------------------------------------------------------- */
    if (data->body == NULL) {
        ci_debug_printf(3, "polis_approval: "
            "approval_process: no body accumulated — "
            "skipping scan\n");
        return CI_MOD_ALLOW204;
    }

    body_ptr = ci_membuf_raw(data->body);
    body_len = (size_t)ci_membuf_size(data->body);

    if (body_ptr == NULL || body_len == 0) {
        ci_debug_printf(3, "polis_approval: "
            "approval_process: empty body — "
            "skipping scan\n");
        return CI_MOD_ALLOW204;
    }

    if (body_len > MAX_BODY_SCAN) {
        ci_debug_printf(3, "polis_approval: "
            "Body size %zu exceeds MAX_BODY_SCAN (%d) — "
            "skipping scan (CWE-400)\n",
            body_len, MAX_BODY_SCAN);
        return CI_MOD_ALLOW204;
    }

    /* ---------------------------------------------------------- */
    /* Step 3: Handle gzip Content-Encoding (Req 2.12)            */
    /* If the response is gzip-compressed, decompress before      */
    /* scanning. We allocate a decompression buffer at 4x the     */
    /* compressed size (capped at MAX_BODY_SCAN).                 */
    /* ---------------------------------------------------------- */
    if (data->is_gzip) {
        int zrc;
        unsigned long try_len;

        /*
         * Estimate decompressed size starting at 4x compressed,
         * retrying with 10x on Z_BUF_ERROR, capped at
         * MAX_BODY_SCAN to prevent decompression bombs.
         * Text-based protocols (JSON/HTML) can compress >4:1,
         * so a single fixed multiplier is insufficient.
         */
        try_len = body_len * 4;
        if (try_len > MAX_BODY_SCAN)
            try_len = MAX_BODY_SCAN;

        decompressed = malloc(try_len);
        if (decompressed == NULL) {
            ci_debug_printf(0, "polis_approval: CRITICAL: "
                "malloc failed for gzip decompression "
                "buffer (%lu bytes)\n",
                (unsigned long)try_len);
            return CI_MOD_ALLOW204;
        }

        decomp_len = try_len;
        zrc = uncompress(decompressed, &decomp_len,
                         (const unsigned char *)body_ptr,
                         (unsigned long)body_len);

        /* Retry with larger buffer if 4x was insufficient */
        if (zrc == Z_BUF_ERROR && try_len < MAX_BODY_SCAN) {
            unsigned long retry_len;
            unsigned char *retry_buf;

            retry_len = body_len * 10;
            if (retry_len > MAX_BODY_SCAN)
                retry_len = MAX_BODY_SCAN;

            ci_debug_printf(3, "polis_approval: "
                "Decompression buffer too small (%lu), "
                "retrying with %lu bytes\n",
                try_len, retry_len);

            retry_buf = realloc(decompressed, retry_len);
            if (retry_buf == NULL) {
                ci_debug_printf(0, "polis_approval: CRITICAL: "
                    "realloc failed for decompression "
                    "retry (%lu bytes)\n", retry_len);
                free(decompressed);
                return CI_MOD_ALLOW204;
            }
            decompressed = retry_buf;
            decomp_len = retry_len;

            zrc = uncompress(decompressed, &decomp_len,
                             (const unsigned char *)body_ptr,
                             (unsigned long)body_len);
        }

        if (zrc != Z_OK) {
            ci_debug_printf(1, "polis_approval: "
                "gzip decompression failed (zlib rc=%d) — "
                "skipping scan\n", zrc);
            free(decompressed);
            return CI_MOD_ALLOW204;
        }

        ci_debug_printf(3, "polis_approval: "
            "Decompressed gzip body: %zu → %lu bytes\n",
            body_len, decomp_len);

        scan_buf = decompressed;
        scan_len = (size_t)decomp_len;
    } else {
        /*
         * Non-gzip: scan the raw body directly.
         * We cast away const because we may modify the body
         * in-place for OTT stripping.
         */
        scan_buf = (unsigned char *)body_ptr;
        scan_len = body_len;
    }

    /* ---------------------------------------------------------- */
    /* Step 4: Scan body for OTT regex pattern                    */
    /* Pattern: ott-[a-zA-Z0-9]{8} (12 chars total)              */
    /* For each match, call process_ott_approval().               */
    /* On successful approval, strip OTT from body (Req 2.10).   */
    /* ---------------------------------------------------------- */
    search_offset = 0;

    while (search_offset < scan_len) {
        char ott_code[OTT_LEN + 1];  /* "ott-XXXXXXXX\0" */
        int approval_rc;
        const char *search_start;

        search_start = (const char *)(scan_buf + search_offset);

        if (regexec(&ott_pattern, search_start,
                    1, &match, 0) != 0) {
            /* No more OTT matches in remaining body */
            break;
        }

        /* Validate match length is exactly OTT_LEN */
        if ((match.rm_eo - match.rm_so) != OTT_LEN) {
            search_offset += (size_t)match.rm_eo;
            continue;
        }

        /* Extract the matched OTT code */
        memcpy(ott_code, search_start + match.rm_so,
               OTT_LEN);
        ott_code[OTT_LEN] = '\0';

        ci_debug_printf(3, "polis_approval: "
            "Found OTT '%s' in body at offset %zu\n",
            ott_code,
            search_offset + (size_t)match.rm_so);

        /* -------------------------------------------------- */
        /* Step 5: Process the OTT approval                   */
        /* Returns: 1=approved, 0=rejected/skip, -1=error     */
        /* -------------------------------------------------- */
        approval_rc = process_ott_approval(ott_code,
                                           data->host);

        if (approval_rc == 1) {
            /*
             * Step 6: Strip OTT from response body (Req 2.10)
             * Replace the OTT code with asterisks of the same
             * length to maintain body size. This prevents the
             * agent from seeing the OTT in the response.
             */
            size_t abs_offset;
            abs_offset = search_offset + (size_t)match.rm_so;

            memset(scan_buf + abs_offset, '*', OTT_LEN);
            body_modified = 1;

            ci_debug_printf(3, "polis_approval: "
                "Stripped OTT '%s' from body "
                "(replaced with asterisks)\n", ott_code);
        } else if (approval_rc < 0) {
            ci_debug_printf(1, "polis_approval: "
                "Error processing OTT '%s' — "
                "continuing scan\n", ott_code);
        }

        /* Advance past this match to find more OTTs */
        search_offset += (size_t)match.rm_eo;
    }

    /* ---------------------------------------------------------- */
    /* Step 7: Recompress if originally gzip-encoded (Req 2.12)   */
    /* Write modified body back to the response.                  */
    /* ---------------------------------------------------------- */
    if (body_modified && data->is_gzip) {
        unsigned char *recompressed = NULL;
        unsigned long recomp_len;
        int zrc;

        /*
         * Allocate recompression buffer. compressBound()
         * gives the maximum compressed size for the input.
         */
        recomp_len = compressBound(decomp_len);
        recompressed = malloc(recomp_len);
        if (recompressed == NULL) {
            ci_debug_printf(0, "polis_approval: CRITICAL: "
                "malloc failed for gzip recompression "
                "buffer (%lu bytes)\n",
                (unsigned long)recomp_len);
            free(decompressed);
            return CI_MOD_ALLOW204;
        }

        zrc = compress(recompressed, &recomp_len,
                       decompressed, decomp_len);
        if (zrc != Z_OK) {
            ci_debug_printf(1, "polis_approval: "
                "gzip recompression failed (zlib rc=%d) — "
                "passing through unmodified\n", zrc);
            free(recompressed);
            free(decompressed);
            return CI_MOD_ALLOW204;
        }

        ci_debug_printf(3, "polis_approval: "
            "Recompressed body: %lu → %lu bytes\n",
            decomp_len, recomp_len);

        /*
         * Replace the accumulated body with the recompressed
         * data. Reset the membuf and write the new content.
         */
        ci_membuf_truncate(data->body, 0);
        ci_membuf_write(data->body,
                        (const char *)recompressed,
                        (int)recomp_len, 1);

        free(recompressed);
        free(decompressed);

        return CI_MOD_DONE;
    }

    if (body_modified && !data->is_gzip) {
        /*
         * Non-gzip body was modified in-place (scan_buf
         * points directly to the membuf data). The membuf
         * already contains the modified content, so we just
         * signal that the body was changed.
         */
        ci_debug_printf(3, "polis_approval: "
            "Body modified in-place "
            "(non-gzip, OTT stripped)\n");
        return CI_MOD_DONE;
    }

    /* Free decompressed buffer if allocated but not modified */
    if (decompressed != NULL)
        free(decompressed);

    if (!body_modified) {
        ci_debug_printf(5, "polis_approval: "
            "No OTT approvals in body from '%s' — "
            "passing through unmodified\n", data->host);
    }

    return CI_MOD_ALLOW204;
}

/*
 * approval_init_request_data - Allocate per-request state for RESPMOD.
 *
 * Creates an approval_req_data_t struct with a ci_membuf for body
 * accumulation. Extracts the Host header from the HTTP *response*
 * headers (not request headers — this is RESPMOD) for domain
 * allowlist checking and context binding. Also checks
 * Content-Encoding for gzip to enable decompression before scan.
 *
 * Returns: pointer to approval_req_data_t, or NULL on failure.
 *
 * Validates: Requirements 2.1
 */
void *approval_init_request_data(ci_request_t *req)
{
    approval_req_data_t *data;
    const char *host_hdr;
    const char *encoding_hdr;

    data = (approval_req_data_t *)malloc(
               sizeof(approval_req_data_t));
    if (!data) {
        ci_debug_printf(1, "polis_approval: ERROR: "
                           "Failed to allocate request data\n");
        return NULL;
    }

    /* Create memory buffer for body accumulation (up to 2MB) */
    data->body = ci_membuf_new_sized(MAX_BODY_SCAN);
    data->total_body_len = 0;
    data->host[0] = '\0';
    data->is_gzip = 0;

    /*
     * Extract Host header from the HTTP response headers.
     * In RESPMOD, the response headers carry the origin
     * server's Host. This is used for domain allowlist
     * checking and context binding verification.
     */
    host_hdr = ci_http_response_get_header(req, "Host");
    if (host_hdr) {
        strncpy(data->host, host_hdr,
                sizeof(data->host) - 1);
        data->host[sizeof(data->host) - 1] = '\0';
        ci_debug_printf(5, "polis_approval: "
                           "Response from host: %s\n",
                        data->host);
    } else {
        ci_debug_printf(5, "polis_approval: "
                           "No Host header in response\n");
    }

    /*
     * Check Content-Encoding for gzip. If the response body
     * is gzip-compressed, we need to decompress before scanning
     * for OTT codes (Req 2.12).
     */
    encoding_hdr = ci_http_response_get_header(
                       req, "Content-Encoding");
    if (encoding_hdr &&
        (strstr(encoding_hdr, "gzip") != NULL)) {
        data->is_gzip = 1;
        ci_debug_printf(5, "polis_approval: "
                           "Response is gzip-encoded\n");
    }

    return data;
}

/*
 * approval_release_request_data - Free per-request data.
 *
 * Called by c-ICAP when a request is complete. Frees the body
 * memory buffer and the request data struct.
 */
void approval_release_request_data(void *data)
{
    approval_req_data_t *req_data = (approval_req_data_t *)data;
    if (!req_data)
        return;

    if (req_data->body) {
        ci_membuf_free(req_data->body);
        req_data->body = NULL;
    }

    free(req_data);
}

/*
 * approval_check_preview - Handle ICAP preview data.
 *
 * Accumulates the preview chunk into the body memory buffer
 * and updates the total body length counter. Returns
 * CI_MOD_CONTINUE to request the full response body.
 */
int approval_check_preview(char *preview_data, int preview_data_len,
                           ci_request_t *req)
{
    approval_req_data_t *data = ci_service_data(req);

    if (!data || !preview_data || preview_data_len <= 0)
        return CI_MOD_CONTINUE;

    ci_membuf_write(data->body, preview_data,
                    preview_data_len, 0);
    data->total_body_len += preview_data_len;

    ci_debug_printf(5, "polis_approval: "
                       "Preview received %d bytes, "
                       "total so far: %zu\n",
                   preview_data_len, data->total_body_len);

    return CI_MOD_CONTINUE;
}

/*
 * approval_io - Handle body data streaming during RESPMOD.
 *
 * Accumulates response body data into the ci_membuf up to
 * MAX_BODY_SCAN (2MB). Bodies exceeding 2MB are simply
 * skipped at scan time (CWE-400).
 *
 * We never modify the response body during streaming (wlen = 0);
 * modification happens in approval_process() after full
 * accumulation.
 *
 * Returns CI_OK on success.
 */
int approval_io(char *wbuf, int *wlen, char *rbuf, int *rlen,
                int iseof, ci_request_t *req)
{
    approval_req_data_t *data = ci_service_data(req);
    int bytes_to_read;
    int membuf_space;
    int membuf_write;

    (void)iseof;

    /* We don't modify the response body during streaming */
    if (wbuf && wlen)
        *wlen = 0;

    if (!data || !rbuf || !rlen || *rlen <= 0)
        return CI_OK;

    bytes_to_read = *rlen;

    /* Accumulate into membuf up to MAX_BODY_SCAN */
    if (data->total_body_len < MAX_BODY_SCAN) {
        membuf_space = MAX_BODY_SCAN
                       - (int)data->total_body_len;
        membuf_write = (bytes_to_read < membuf_space)
                       ? bytes_to_read : membuf_space;
        ci_membuf_write(data->body, rbuf,
                        membuf_write, 0);
    }

    /* Always track total body length for the size check */
    data->total_body_len += bytes_to_read;

    return CI_OK;
}

/*
 * approval_close_service - Clean up when the approval service shuts down.
 *
 * Frees the compiled OTT regex pattern and disconnects from
 * Valkey to avoid resource leaks.
 */
void approval_close_service(void)
{
    ci_debug_printf(3, "polis_approval: "
                       "Closing service\n");

    /* Free the compiled OTT regex pattern */
    regfree(&ott_pattern);

    /* Disconnect from Valkey if connected */
    if (valkey_ctx) {
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
    }

    ci_debug_printf(3, "polis_approval: "
                       "Service closed, resources freed\n");
}
