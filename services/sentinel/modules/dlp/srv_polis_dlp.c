/*
 * srv_polis_dlp.c - c-ICAP DLP module for credential detection
 *
 * REQMOD service that scans outbound HTTP request bodies for credential
 * patterns and blocks exfiltration to unauthorized destinations.
 */

/* c-ICAP headers */
#include "c_icap/c-icap.h"
#include "c_icap/service.h"
#include "c_icap/header.h"
#include "c_icap/body.h"
#include "c_icap/simple_api.h"
#include "c_icap/request_util.h"

/* Standard library headers */
#include <regex.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>

/* Valkey/Redis client */
#include <hiredis/hiredis.h>
#include <hiredis/hiredis_ssl.h>

/* Constants */
#define MAX_PATTERNS    32
#define MAX_PATTERN_LEN 256
#define MAX_BODY_SCAN   1048576   /* 1MB main body scan limit */
#define TAIL_SCAN_SIZE  10240     /* 10KB tail scan for padding bypass prevention */

/*
 * dlp_pattern_t - A single credential detection pattern with its
 * associated allow rule and blocking behavior.
 */
typedef struct {
    char name[64];              /* Pattern name (e.g., "anthropic") */
    regex_t regex;              /* Compiled credential regex */
    char allow_domain[256];     /* Expected destination domain regex (empty = always block) */
    regex_t allow_regex;        /* Pre-compiled allow domain regex */
    int allow_compiled;         /* 1 if allow_regex was successfully compiled */
    int always_block;           /* 1 if pattern should always block (e.g., private keys) */
} dlp_pattern_t;

/*
 * dlp_req_data_t - Per-request state for body accumulation and
 * scan results during REQMOD processing.
 */
typedef struct {
    ci_membuf_t *body;          /* Accumulated request body (first 1MB) */
    ci_cached_file_t *ring;     /* Cached file for body pass-through (mem → disk) */
    ci_membuf_t *error_page;    /* Error page body for blocked responses */
    char tail[TAIL_SCAN_SIZE];  /* Last 10KB ring buffer for tail scan */
    size_t tail_len;            /* Bytes currently in tail buffer */
    size_t total_body_len;      /* Total body length seen so far */
    char host[256];             /* Host header value from request */
    int blocked;                /* Whether this request was blocked */
    char matched_pattern[64];   /* Name of the pattern that matched */
    int eof;                    /* End of data received */
    size_t error_page_sent;     /* Bytes of error page already sent */
    int ott_rewritten;          /* OTT substitution was performed */
    size_t ott_body_sent;       /* Bytes of OTT-rewritten body already sent */
    char request_id[16];        /* Generated request ID for blocked requests */
} dlp_req_data_t;

/* Static pattern storage - loaded from config at service init */
static dlp_pattern_t patterns[MAX_PATTERNS];
static int pattern_count = 0;

/*
 * Security level enum — maps to Valkey values at polis:config:security_level.
 * Controls DLP behavior for new (unknown) domains.
 */
typedef enum {
    LEVEL_RELAXED  = 0,   /* New domains: auto-allow */
    LEVEL_BALANCED = 1,   /* New domains: HITL prompt (default) */
    LEVEL_STRICT   = 2    /* New domains: block */
} security_level_t;

/* Valkey polling constants */
#define LEVEL_POLL_INTERVAL 1      /* Requests between Valkey polls */
#define LEVEL_POLL_MAX      10000  /* Max backoff interval (requests) */

/* Security level state — Valkey connection and polling */
static redisContext *valkey_level_ctx = NULL;
static security_level_t current_level = LEVEL_BALANCED;
static unsigned long request_counter = 0;
static unsigned long current_poll_interval = LEVEL_POLL_INTERVAL;

/*
 * Mutex protecting all Valkey-related shared state:
 * valkey_level_ctx, current_level, request_counter, current_poll_interval.
 * c-ICAP uses MPMT (multi-threaded) model — concurrent requests call
 * apply_security_policy() from different threads. hiredis contexts are
 * NOT thread-safe, so all access must be serialized.
 */
static pthread_mutex_t valkey_mutex = PTHREAD_MUTEX_INITIALIZER;

/* --- OTT rewrite additions --- */
static regex_t approve_pattern;           /* /polis-approve req-* regex */
static int time_gate_secs = 15;           /* Time-gate delay (seconds) */
static int ott_ttl_secs = 600;            /* OTT key TTL in Valkey */
static redisContext *valkey_gov_ctx = NULL;/* governance-reqmod connection */
static pthread_mutex_t gov_valkey_mutex = PTHREAD_MUTEX_INITIALIZER;

/* OTT generation constants */
#define OTT_LEN         12        /* "ott-" + 8 alphanumeric chars */
#define OTT_RANDOM_BYTES 8        /* Random bytes needed for 8 alphanumeric chars */

/* 62-char alphanumeric alphabet for OTT code characters */
static const char OTT_CHARSET[] =
    "abcdefghijklmnopqrstuvwxyz"
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    "0123456789";
#define OTT_CHARSET_LEN 62  /* sizeof(OTT_CHARSET) - 1 */

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

/*
 * gov_valkey_init - Initialize governance-reqmod Valkey connection.
 *
 * Establishes a TLS connection to Valkey as the governance-reqmod user
 * for OTT storage and approval operations. This is a separate connection
 * from the existing dlp-reader connection.
 *
 * Reads password from /run/secrets/valkey_reqmod_password.
 *
 * Returns 0 on success, -1 on failure.
 */
static int gov_valkey_init(void)
{
    const char *vk_host;
    int vk_port = 6379;
    redisSSLContext *ssl_ctx = NULL;
    redisSSLContextError ssl_err;
    redisReply *reply;
    FILE *fp;
    char password[256];
    size_t pass_len;

    /* Lock: all governance Valkey state modifications under mutex */
    pthread_mutex_lock(&gov_valkey_mutex);

    /* Read Valkey host from environment (default: "state") */
    vk_host = getenv("polis_VALKEY_HOST");
    if (!vk_host) vk_host = "state";

    /* Initialize OpenSSL for hiredis TLS */
    redisInitOpenSSL();

    /* Create TLS context with client certificates for mTLS */
    ssl_ctx = redisCreateSSLContext(
        "/etc/valkey/tls/ca.crt",
        NULL,   /* capath — not used, single CA file */
        "/etc/valkey/tls/client.crt",
        "/etc/valkey/tls/client.key",
        NULL,   /* server_name — use default */
        &ssl_err);
    if (ssl_ctx == NULL) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "Failed to create TLS context for governance-reqmod: %s — "
            "OTT rewriting unavailable\n",
            redisSSLContextGetError(ssl_err));
        pthread_mutex_unlock(&gov_valkey_mutex);
        return -1;
    }

    /* Establish TCP connection to Valkey */
    valkey_gov_ctx = redisConnect(vk_host, vk_port);
    if (valkey_gov_ctx == NULL ||
        valkey_gov_ctx->err) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "Cannot connect to Valkey at %s:%d for governance-reqmod%s%s — "
            "OTT rewriting unavailable\n",
            vk_host, vk_port,
            valkey_gov_ctx ? ": " : "",
            valkey_gov_ctx ? valkey_gov_ctx->errstr : "");
        if (valkey_gov_ctx) {
            redisFree(valkey_gov_ctx);
            valkey_gov_ctx = NULL;
        }
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&gov_valkey_mutex);
        return -1;
    }

    /* Initiate TLS handshake on the connection */
    if (redisInitiateSSLWithContext(valkey_gov_ctx,
                                    ssl_ctx) != REDIS_OK) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "TLS handshake failed with Valkey for governance-reqmod: %s — "
            "OTT rewriting unavailable\n",
            valkey_gov_ctx->errstr);
        redisFree(valkey_gov_ctx);
        valkey_gov_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&gov_valkey_mutex);
        return -1;
    }

    /* Read governance-reqmod password from Docker secret file */
    fp = fopen("/run/secrets/valkey_reqmod_password", "r");
    if (!fp) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "Cannot open /run/secrets/valkey_reqmod_password — "
            "OTT rewriting unavailable\n");
        redisFree(valkey_gov_ctx);
        valkey_gov_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&gov_valkey_mutex);
        return -1;
    }

    memset(password, 0, sizeof(password));
    if (fgets(password, sizeof(password), fp) == NULL) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "Failed to read password from "
            "/run/secrets/valkey_reqmod_password\n");
        fclose(fp);
        memset(password, 0, sizeof(password));
        redisFree(valkey_gov_ctx);
        valkey_gov_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&gov_valkey_mutex);
        return -1;
    }
    fclose(fp);

    /* Strip trailing newline from password */
    pass_len = strlen(password);
    while (pass_len > 0 &&
           (password[pass_len - 1] == '\n' ||
            password[pass_len - 1] == '\r')) {
        password[--pass_len] = '\0';
    }

    /* Authenticate with ACL: AUTH governance-reqmod <password> */
    reply = redisCommand(valkey_gov_ctx,
        "AUTH governance-reqmod %s", password);

    /* Scrub password from stack immediately after AUTH */
    memset(password, 0, sizeof(password));

    if (reply == NULL ||
        reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "polis_dlp: CRITICAL: "
            "Valkey ACL auth failed as governance-reqmod%s%s — "
            "OTT rewriting unavailable\n",
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        redisFree(valkey_gov_ctx);
        valkey_gov_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&gov_valkey_mutex);
        return -1;
    }
    freeReplyObject(reply);

    ci_debug_printf(3, "polis_dlp: "
        "Authenticated as governance-reqmod\n");

    ci_debug_printf(3, "polis_dlp: "
        "Connected to Valkey at %s:%d as governance-reqmod (TLS + ACL)\n",
        vk_host, vk_port);

    redisFreeSSLContext(ssl_ctx);

    pthread_mutex_unlock(&gov_valkey_mutex);
    return 0;
}

/*
 * ensure_gov_valkey_connected - Lazy reconnect for governance-reqmod.
 *
 * Checks if valkey_gov_ctx is connected. If not, attempts reconnection
 * via gov_valkey_init(). Thread-safe via gov_valkey_mutex.
 *
 * Returns 1 if connected, 0 if unavailable.
 */
static int ensure_gov_valkey_connected(void)
{
    int connected;

    pthread_mutex_lock(&gov_valkey_mutex);

    /* Check if context exists and is not in error state */
    if (valkey_gov_ctx != NULL && valkey_gov_ctx->err == 0) {
        /* Test connection with PING */
        redisReply *reply = redisCommand(valkey_gov_ctx, "PING");
        if (reply != NULL && reply->type != REDIS_REPLY_ERROR) {
            freeReplyObject(reply);
            pthread_mutex_unlock(&gov_valkey_mutex);
            return 1;  /* Connected */
        }
        if (reply) freeReplyObject(reply);

        /* PING failed — connection is stale, free it */
        ci_debug_printf(2, "polis_dlp: "
            "governance-reqmod connection stale, reconnecting\n");
        redisFree(valkey_gov_ctx);
        valkey_gov_ctx = NULL;
    }

    pthread_mutex_unlock(&gov_valkey_mutex);

    /* Attempt reconnection (gov_valkey_init handles its own locking) */
    connected = (gov_valkey_init() == 0);

    return connected;
}

/*
 * find_pattern_by_name - Look up a loaded pattern by its name.
 * Returns pointer to the pattern, or NULL if not found.
 */
static dlp_pattern_t *find_pattern_by_name(const char *name)
{
    int i;
    for (i = 0; i < pattern_count; i++) {
        if (strcmp(patterns[i].name, name) == 0)
            return &patterns[i];
    }
    return NULL;
}

/* Forward declaration for lazy Valkey init from refresh_security_level */
static int dlp_valkey_init_locked(void);

/*
 * refresh_security_level - Poll Valkey for the current security level.
 *
 * Executes GET polis:config:security_level via hiredis. On success,
 * parses the value (handling both "relaxed" and relaxed — with or
 * without JSON quotes) and updates current_level. Unknown values
 * default to LEVEL_BALANCED.
 *
 * On failure: keeps current_level unchanged, doubles the poll
 * interval (exponential backoff, capped at LEVEL_POLL_MAX), and
 * logs the new backoff value.
 *
 * On success: resets current_poll_interval to LEVEL_POLL_INTERVAL.
 *
 * Requirements: 1.3, 1.4, 1.5, 1.6
 */
static void refresh_security_level(void)
{
    redisReply *reply;
    const char *val;
    char stripped[64];
    size_t len;

    /* Lazy connect: if no Valkey connection, try to establish one.
     * This handles the MPMT fork case where connections established
     * in the main process are invalid in child processes. */
    if (valkey_level_ctx == NULL) {
        if (dlp_valkey_init_locked() != 0)
            return;  /* Still can't connect */
    }

    reply = redisCommand(valkey_level_ctx,
                         "GET polis:config:security_level");

    /* Failure path: free stale connection, try reconnect */
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        if (reply)
            freeReplyObject(reply);
        /* Connection is stale — free and try reconnect */
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
        current_poll_interval *= 2;
        if (current_poll_interval > LEVEL_POLL_MAX)
            current_poll_interval = LEVEL_POLL_MAX;
        ci_debug_printf(1, "polis_dlp: Valkey poll failed, "
                           "keeping level %d, next poll in "
                           "%lu requests\n",
                       (int)current_level,
                       current_poll_interval);
        return;
    }

    /* Success: reset poll interval */
    current_poll_interval = LEVEL_POLL_INTERVAL;

    /* NIL reply (key not set) — default to balanced */
    if (reply->type == REDIS_REPLY_NIL || reply->str == NULL) {
        current_level = LEVEL_BALANCED;
        freeReplyObject(reply);
        return;
    }

    /*
     * Strip leading/trailing '"' from the value.
     * The CLI uses serde_json::to_string() which wraps the
     * value in JSON quotes: "\"relaxed\"" stored in Valkey.
     */
    val = reply->str;
    len = strlen(val);
    if (len >= 2 && val[0] == '"' && val[len - 1] == '"') {
        /* Copy without surrounding quotes */
        if (len - 2 < sizeof(stripped)) {
            memcpy(stripped, val + 1, len - 2);
            stripped[len - 2] = '\0';
        } else {
            stripped[0] = '\0';
        }
        val = stripped;
    }

    /* Map string value to security_level_t enum */
    if (strcasecmp(val, "relaxed") == 0) {
        current_level = LEVEL_RELAXED;
    } else if (strcasecmp(val, "balanced") == 0) {
        current_level = LEVEL_BALANCED;
    } else if (strcasecmp(val, "strict") == 0) {
        current_level = LEVEL_STRICT;
    } else {
        /* Unknown value — default to balanced */
        ci_debug_printf(1, "polis_dlp: Unknown security level "
                           "'%s', defaulting to balanced\n",
                       val);
        current_level = LEVEL_BALANCED;
    }

    ci_debug_printf(5, "polis_dlp: Security level updated "
                       "to %d\n", (int)current_level);

    freeReplyObject(reply);
}

/*
 * is_new_domain - Check if a host is a known-good domain.
 *
 * Uses dot-boundary suffix matching to prevent CWE-346
 * substring spoofing. Known domains are stored with a
 * leading dot (e.g., ".github.com") so that:
 *   - "api.github.com" matches (ends with ".github.com")
 *   - "evil-github.com" does NOT match (no dot boundary)
 *   - "github.com" matches via exact match (domain + 1)
 *
 * Returns 0 if the host is a known domain, 1 if new.
 */
static int is_new_domain(const char *host)
{
    static const char *known_domains[] = {
        ".api.anthropic.com",
        ".api.openai.com",
        ".api.github.com",
        ".github.com",
        ".amazonaws.com",
        ".api.telegram.org",
        ".discord.com",
        ".api.slack.com",
        NULL
    };
    size_t hlen, dlen;
    int i;

    if (host == NULL || host[0] == '\0')
        return 1;

    hlen = strlen(host);

    for (i = 0; known_domains[i] != NULL; i++) {
        dlen = strlen(known_domains[i]);

        /* Suffix match: host ends with ".domain.com" */
        if (hlen >= dlen &&
            strcasecmp(host + (hlen - dlen),
                       known_domains[i]) == 0) {
            return 0;  /* known domain */
        }

        /* Exact match without leading dot */
        if (strcasecmp(host, known_domains[i] + 1) == 0) {
            return 0;  /* known domain */
        }
    }

    return 1;  /* new domain */
}

/*
 * apply_security_policy - Per-request policy decision.
 *
 * Increments the request counter and polls Valkey for security level
 * changes every current_poll_interval requests. Then evaluates the
 * request against the active security level:
 *
 *   - Credentials always trigger a HITL prompt (return 1) regardless
 *     of security level (Requirement 2.4).
 *   - New domains: RELAXED → allow (0), BALANCED → prompt (1),
 *     STRICT → block (2).
 *   - Known domains with no credential → allow (0).
 *
 * Returns: 0 = allow, 1 = prompt (HITL), 2 = block.
 *
 * Requirements: 2.1, 2.2, 2.3, 2.4, 2.5
 */
static int apply_security_policy(const char *host, int has_credential)
{
    int new_domain;
    security_level_t level_snapshot;

    /* Lock: increment counter, poll if needed, snapshot level */
    pthread_mutex_lock(&valkey_mutex);
    request_counter++;
    if (request_counter % current_poll_interval == 0) {
        refresh_security_level();
    }
    level_snapshot = current_level;
    pthread_mutex_unlock(&valkey_mutex);

    /* Credentials always trigger a HITL prompt at any level */
    if (has_credential) {
        return 1;  /* prompt */
    }

    /* Check if this is a new (unknown) domain */
    new_domain = is_new_domain(host);

    if (!new_domain) {
        return 0;  /* known domain, no credential → allow */
    }

    /* New domain: behavior depends on current security level */
    switch (level_snapshot) {
    case LEVEL_RELAXED:
        return 0;  /* auto-allow new domains */
    case LEVEL_BALANCED:
        return 1;  /* prompt for new domains */
    case LEVEL_STRICT:
        return 2;  /* block new domains */
    default:
        return 1;  /* unknown level → treat as balanced */
    }
}

/*
 * dlp_valkey_init - Connect to Valkey as dlp-reader with TLS + ACL.
 *
 * Reads polis_VALKEY_HOST env var (default: "valkey"), port 6379.
 * Creates TLS context with CA, client cert, client key from
 * /etc/valkey/tls/. Reads password from Docker secret file at
 * /run/secrets/valkey_dlp_password, strips trailing newline,
 * authenticates as dlp-reader, then scrubs password from stack.
 * Calls refresh_security_level() for initial level read.
 *
 * Returns 0 on success, -1 on any failure.
 *
 * Requirements: 1.1, 1.2, 1.7, 1.8
 */
/*
 * dlp_valkey_init_locked - Inner function for dlp-reader Valkey connection.
 *
 * Same as dlp_valkey_init() but assumes valkey_mutex is already held.
 * Used for lazy initialization from refresh_security_level().
 */
static int dlp_valkey_init_locked(void)
{
    const char *vk_host;
    int vk_port = 6379;
    redisSSLContext *ssl_ctx = NULL;
    redisSSLContextError ssl_err;
    redisReply *reply;
    FILE *fp;
    char password[256];
    size_t pass_len;

    /* Free stale connection if any */
    if (valkey_level_ctx) {
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
    }

    /* Read Valkey host from environment (default: "state") */
    vk_host = getenv("polis_VALKEY_HOST");
    if (!vk_host) vk_host = "state";

    /* Initialize OpenSSL for hiredis TLS */
    redisInitOpenSSL();

    /* Create TLS context with client certificates for mTLS */
    ssl_ctx = redisCreateSSLContext(
        "/etc/valkey/tls/ca.crt",
        NULL,
        "/etc/valkey/tls/client.crt",
        "/etc/valkey/tls/client.key",
        NULL,
        &ssl_err);
    if (ssl_ctx == NULL) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "Failed to create TLS context: %s — "
            "Valkey connection unavailable\n",
            redisSSLContextGetError(ssl_err));
        return -1;
    }

    /* Establish TCP connection to Valkey */
    valkey_level_ctx = redisConnect(vk_host, vk_port);
    if (valkey_level_ctx == NULL ||
        valkey_level_ctx->err) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "Cannot connect to Valkey at %s:%d%s%s\n",
            vk_host, vk_port,
            valkey_level_ctx ? ": " : "",
            valkey_level_ctx ? valkey_level_ctx->errstr : "");
        if (valkey_level_ctx) {
            redisFree(valkey_level_ctx);
            valkey_level_ctx = NULL;
        }
        redisFreeSSLContext(ssl_ctx);
        return -1;
    }

    /* Initiate TLS handshake */
    if (redisInitiateSSLWithContext(valkey_level_ctx,
                                    ssl_ctx) != REDIS_OK) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "TLS handshake failed: %s\n",
            valkey_level_ctx->errstr);
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        return -1;
    }

    /* Read dlp-reader password from Docker secret file */
    fp = fopen("/run/secrets/valkey_dlp_password", "r");
    if (!fp) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "Cannot open /run/secrets/valkey_dlp_password\n");
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        return -1;
    }

    memset(password, 0, sizeof(password));
    if (fgets(password, sizeof(password), fp) == NULL) {
        ci_debug_printf(1, "polis_dlp: WARNING: "
            "Failed to read dlp-reader password\n");
        fclose(fp);
        memset(password, 0, sizeof(password));
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        return -1;
    }
    fclose(fp);

    /* Strip trailing newline */
    pass_len = strlen(password);
    while (pass_len > 0 &&
           (password[pass_len - 1] == '\n' ||
            password[pass_len - 1] == '\r')) {
        password[--pass_len] = '\0';
    }

    /* Authenticate with ACL */
    reply = redisCommand(valkey_level_ctx,
        "AUTH dlp-reader %s", password);
    memset(password, 0, sizeof(password));

    if (reply == NULL ||
        reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "polis_dlp: CRITICAL: "
            "Valkey ACL auth failed as dlp-reader%s%s\n",
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        return -1;
    }
    freeReplyObject(reply);

    ci_debug_printf(1, "polis_dlp: "
        "Connected to Valkey at %s:%d as dlp-reader "
        "(TLS + ACL)\n", vk_host, vk_port);

    redisFreeSSLContext(ssl_ctx);
    return 0;
}

/* Forward declarations for service callbacks */
int dlp_init_service(ci_service_xdata_t *srv_xdata,
                     struct ci_server_conf *server_conf);
void dlp_close_service(void);
void *dlp_init_request_data(ci_request_t *req);
void dlp_release_request_data(void *data);
int dlp_check_preview(char *preview_data, int preview_data_len,
                      ci_request_t *req);
int dlp_process(ci_request_t *req);
int dlp_io(char *wbuf, int *wlen, char *rbuf, int *rlen,
           int iseof, ci_request_t *req);

/*
 * Service module definition - exported to c-ICAP.
 * Registers the DLP module as a REQMOD service named "polis_dlp".
 */
CI_DECLARE_MOD_DATA ci_service_module_t service = {
    "polis_dlp",                     /* mod_name */
    "polis DLP credential detection service",  /* mod_short_descr */
    ICAP_REQMOD,                     /* mod_type */
    dlp_init_service,                /* mod_init_service */
    NULL,                            /* mod_post_init_service */
    dlp_close_service,               /* mod_close_service */
    dlp_init_request_data,           /* mod_init_request_data */
    dlp_release_request_data,        /* mod_release_request_data */
    dlp_check_preview,               /* mod_check_preview_handler */
    dlp_process,                     /* mod_end_of_data_handler */
    dlp_io,                          /* mod_service_io */
    NULL,                            /* mod_conf_table */
    NULL                             /* mod_data */
};

/*
 * dlp_init_service - Initialize the DLP service.
 *
 * Parses /etc/c-icap/polis_dlp.conf to load credential patterns,
 * allow rules, and action directives. Sets preview size and enables
 * 204 responses for the ICAP service.
 *
 * Returns CI_OK on success.
 */
int dlp_init_service(ci_service_xdata_t *srv_xdata,
                     struct ci_server_conf *server_conf)
{
    FILE *fp;
    char line[1024];
    char name[64];
    char value[MAX_PATTERN_LEN];
    dlp_pattern_t *pat;

    /* Configure ICAP service parameters */
    ci_service_set_preview(srv_xdata, 4096);
    ci_service_enable_204(srv_xdata);

    pattern_count = 0;

    ci_debug_printf(3, "polis_dlp: Initializing service, "
                       "loading config from /etc/c-icap/polis_dlp.conf\n");

    fp = fopen("/etc/c-icap/polis_dlp.conf", "r");
    if (!fp) {
        ci_debug_printf(0, "polis_dlp: CRITICAL: Cannot open config file "
                           "/etc/c-icap/polis_dlp.conf — refusing to start\n");
        return CI_ERROR;
    }

    while (fgets(line, sizeof(line), fp)) {
        /* Strip trailing newline */
        size_t len = strlen(line);
        while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r'))
            line[--len] = '\0';

        /* Skip blank lines and comments */
        if (len == 0 || line[0] == '#')
            continue;

        /* Parse pattern.<name> = <regex> */
        if (sscanf(line, " pattern.%63[^ ] = %255[^\n]", name, value) == 2) {
            if (pattern_count >= MAX_PATTERNS) {
                ci_debug_printf(1, "polis_dlp: WARNING: Max patterns (%d) "
                                   "reached, skipping '%s'\n",
                                MAX_PATTERNS, name);
                continue;
            }
            if (regcomp(&patterns[pattern_count].regex, value,
                        REG_EXTENDED | REG_NOSUB) != 0) {
                ci_debug_printf(1, "polis_dlp: ERROR: Failed to compile "
                                   "regex for pattern '%s'\n", name);
                continue;
            }
            strncpy(patterns[pattern_count].name, name,
                    sizeof(patterns[pattern_count].name) - 1);
            patterns[pattern_count].name[
                sizeof(patterns[pattern_count].name) - 1] = '\0';
            patterns[pattern_count].allow_domain[0] = '\0';
            patterns[pattern_count].allow_compiled = 0;
            patterns[pattern_count].always_block = 0;
            ci_debug_printf(3, "polis_dlp: Loaded pattern '%s'\n", name);
            pattern_count++;
            continue;
        }

        /* Parse allow.<name> = <domain_regex> */
        if (sscanf(line, " allow.%63[^ ] = %255[^\n]", name, value) == 2) {
            pat = find_pattern_by_name(name);
            if (pat) {
                strncpy(pat->allow_domain, value,
                        sizeof(pat->allow_domain) - 1);
                pat->allow_domain[sizeof(pat->allow_domain) - 1] = '\0';
                /* Pre-compile the allow domain regex at init time */
                if (regcomp(&pat->allow_regex, value,
                            REG_EXTENDED | REG_NOSUB) == 0) {
                    pat->allow_compiled = 1;
                    ci_debug_printf(3, "polis_dlp: Set allow domain for "
                                       "'%s': %s\n", name, value);
                } else {
                    pat->allow_compiled = 0;
                    ci_debug_printf(1, "polis_dlp: ERROR: Failed to compile "
                                       "allow regex for '%s'\n", name);
                }
            } else {
                ci_debug_printf(1, "polis_dlp: WARNING: Allow rule for "
                                   "unknown pattern '%s'\n", name);
            }
            continue;
        }

        /* Parse action.<name> = block */
        if (sscanf(line, " action.%63[^ ] = %255[^\n]", name, value) == 2) {
            pat = find_pattern_by_name(name);
            if (pat && strcmp(value, "block") == 0) {
                pat->always_block = 1;
                ci_debug_printf(3, "polis_dlp: Set always_block for "
                                   "'%s'\n", name);
            } else if (!pat) {
                ci_debug_printf(1, "polis_dlp: WARNING: Action for "
                                   "unknown pattern '%s'\n", name);
            }
            continue;
        }
    }

    fclose(fp);

    ci_debug_printf(3, "polis_dlp: Initialization complete, "
                       "%d patterns loaded\n", pattern_count);

    /* Fail-closed: refuse to start if no credential patterns loaded (CWE-636) */
    if (pattern_count == 0) {
        ci_debug_printf(0, "polis_dlp: CRITICAL: No credential patterns "
                           "loaded from polis_dlp.conf — refusing to start "
                           "(fail-closed, CWE-636)\n");
        return CI_ERROR;
    }

    /* Valkey connections are lazy-initialized on first use in child
     * processes. c-ICAP uses MPMT (pre-fork) model — connections
     * established here in the main process would be corrupted after
     * fork because OpenSSL/TLS state is not fork-safe. */
    ci_debug_printf(3, "polis_dlp: Valkey connections will be "
                       "lazy-initialized on first use\n");

    /* --- OTT rewrite initialization (Requirements 1.3, 1.9, 1.12) --- */
    
    /* Compile approve pattern regex: /polis-approve req-{hex8} */
    int rc = regcomp(&approve_pattern,
                     "/polis-approve[[:space:]]+(req-[a-f0-9]{8})",
                     REG_EXTENDED);
    if (rc != 0) {
        char errbuf[128];
        regerror(rc, &approve_pattern, errbuf, sizeof(errbuf));
        ci_debug_printf(0, "polis_dlp: CRITICAL: Failed to compile "
                           "approve pattern regex: %s\n", errbuf);
        return CI_ERROR;
    }
    ci_debug_printf(3, "polis_dlp: Compiled approve pattern regex\n");

    /* Load time-gate duration from environment (Requirement 1.12) */
    const char *env_val = getenv("POLIS_APPROVAL_TIME_GATE_SECS");
    if (env_val != NULL) {
        int parsed = atoi(env_val);
        if (parsed > 0) {
            time_gate_secs = parsed;
            ci_debug_printf(3, "polis_dlp: time_gate_secs set to %d from env\n",
                            time_gate_secs);
        } else {
            ci_debug_printf(1, "polis_dlp: WARNING: invalid "
                               "POLIS_APPROVAL_TIME_GATE_SECS='%s', "
                               "using default %d\n", env_val, time_gate_secs);
        }
    } else {
        ci_debug_printf(3, "polis_dlp: POLIS_APPROVAL_TIME_GATE_SECS not set, "
                           "using default %d\n", time_gate_secs);
    }

    /* governance-reqmod Valkey is also lazy-initialized (see above) */

    ci_debug_printf(3, "polis_dlp: OTT rewrite initialization complete "
                       "(time_gate=%ds, ott_ttl=%ds)\n",
                    time_gate_secs, ott_ttl_secs);

    return CI_OK;
}

/*
 * dlp_close_service - Clean up when the DLP service is shut down.
 *
 * Frees all compiled regex patterns to avoid memory leaks.
 */
void dlp_close_service(void)
{
    int i;
    ci_debug_printf(3, "polis_dlp: Closing service, "
                       "freeing %d patterns\n", pattern_count);
    for (i = 0; i < pattern_count; i++) {
        regfree(&patterns[i].regex);
        if (patterns[i].allow_compiled)
            regfree(&patterns[i].allow_regex);
    }
    pattern_count = 0;

    /* Tear down Valkey connection under lock */
    pthread_mutex_lock(&valkey_mutex);
    if (valkey_level_ctx) {
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
    }
    pthread_mutex_unlock(&valkey_mutex);
    pthread_mutex_destroy(&valkey_mutex);
    
    /* Tear down governance-reqmod Valkey connection */
    pthread_mutex_lock(&gov_valkey_mutex);
    if (valkey_gov_ctx) {
        redisFree(valkey_gov_ctx);
        valkey_gov_ctx = NULL;
    }
    pthread_mutex_unlock(&gov_valkey_mutex);
    pthread_mutex_destroy(&gov_valkey_mutex);
    
    /* Free OTT regex */
    regfree(&approve_pattern);
}

/*
 * dlp_init_request_data - Allocate and initialize per-request data.
 *
 * Called by c-ICAP for each new REQMOD request. Allocates the
 * dlp_req_data_t struct, creates a memory buffer for body
 * accumulation, and extracts the Host header from the request.
 *
 * Returns pointer to the allocated request data, or NULL on failure.
 */
void *dlp_init_request_data(ci_request_t *req)
{
    dlp_req_data_t *data;
    const char *host_hdr;

    data = (dlp_req_data_t *)malloc(sizeof(dlp_req_data_t));
    if (!data) {
        ci_debug_printf(1, "polis_dlp: ERROR: Failed to allocate "
                           "request data\n");
        return NULL;
    }

    /* Create memory buffer for body accumulation (up to 1MB) */
    data->body = ci_membuf_new_sized(MAX_BODY_SCAN);
    /* Create cached file for body pass-through.
     * Uses ci_cached_file_t: starts in memory (up to CI_BODY_MAX_MEM,
     * typically 128KB), then spills to a temp file on disk for larger
     * bodies. Handles arbitrarily large AI agent prompts without the
     * fixed-size overflow problem of ci_ring_buf_t. */
    data->ring = ci_req_hasbody(req) ? ci_cached_file_new(CI_BODY_MAX_MEM) : NULL;
    data->error_page = NULL;
    data->tail_len = 0;
    data->total_body_len = 0;
    data->host[0] = '\0';
    data->blocked = 0;
    data->matched_pattern[0] = '\0';
    data->eof = 0;
    data->error_page_sent = 0;
    data->ott_rewritten = 0;
    data->ott_body_sent = 0;
    memset(data->tail, 0, TAIL_SCAN_SIZE);

    /* Extract Host header from the HTTP request */
    host_hdr = ci_http_request_get_header(req, "Host");
    if (host_hdr) {
        strncpy(data->host, host_hdr, sizeof(data->host) - 1);
        data->host[sizeof(data->host) - 1] = '\0';
        ci_debug_printf(5, "polis_dlp: Request to host: %s\n",
                        data->host);
    } else {
        ci_debug_printf(5, "polis_dlp: No Host header found\n");
    }

    return data;
}

/*
 * dlp_release_request_data - Free per-request data.
 *
 * Called by c-ICAP when a request is complete. Frees the body
 * memory buffer and the request data struct.
 */
void dlp_release_request_data(void *data)
{
    dlp_req_data_t *req_data = (dlp_req_data_t *)data;
    if (!req_data)
        return;

    if (req_data->body) {
        ci_membuf_free(req_data->body);
        req_data->body = NULL;
    }

    if (req_data->ring) {
        ci_cached_file_destroy(req_data->ring);
        req_data->ring = NULL;
    }

    if (req_data->error_page) {
        ci_membuf_free(req_data->error_page);
        req_data->error_page = NULL;
    }

    free(req_data);
}

/*
 * check_patterns - Scan a body buffer against all loaded DLP patterns.
 *
 * Iterates through all loaded credential patterns and checks the body
 * for matches. For each match:
 *   - If always_block is set, the request is blocked immediately.
 *   - If an allow_domain is configured, the host is checked against it.
 *     If the host matches the allow rule, scanning continues to the
 *     next pattern. If the host does NOT match, the request is blocked.
 *   - If no allow_domain is set (and not always_block), the request
 *     is blocked (default action).
 *
 * Parameters:
 *   body     - Pointer to the null-terminated body buffer to scan
 *   body_len - Length of the body buffer (unused; body is null-terminated)
 *   data     - Per-request data containing host and result fields
 *
 * Returns 1 if a credential was detected and the request should be
 * blocked, 0 if no actionable matches were found.
 */
static int check_patterns(const char *body, int body_len,
                          dlp_req_data_t *data)
{
    int i;

    (void)body_len; /* body is null-terminated from ci_membuf */

    for (i = 0; i < pattern_count; i++) {
        /* Test this pattern against the body */
        if (regexec(&patterns[i].regex, body, 0, NULL, 0) != 0)
            continue;

        /* Pattern matched - check blocking rules */
        ci_debug_printf(3, "polis_dlp: Pattern '%s' matched\n",
                        patterns[i].name);

        /* Always-block patterns (e.g., private keys) */
        if (patterns[i].always_block) {
            data->blocked = 1;
            strncpy(data->matched_pattern, patterns[i].name,
                    sizeof(data->matched_pattern) - 1);
            data->matched_pattern[
                sizeof(data->matched_pattern) - 1] = '\0';
            ci_debug_printf(3, "polis_dlp: Blocked by always_block "
                               "pattern '%s'\n", patterns[i].name);
            return 1;
        }

        /* Pattern has a pre-compiled allow_domain - check host against it */
        if (patterns[i].allow_compiled) {
            if (regexec(&patterns[i].allow_regex, data->host,
                        0, NULL, 0) == 0) {
                /* Host matches allow rule - credential going to
                   expected destination, continue scanning */
                ci_debug_printf(3, "polis_dlp: Pattern '%s' "
                                   "allowed for host '%s'\n",
                               patterns[i].name, data->host);
                continue;
            }
            /* Host does NOT match allow rule - block */
            data->blocked = 1;
            strncpy(data->matched_pattern, patterns[i].name,
                    sizeof(data->matched_pattern) - 1);
            data->matched_pattern[
                sizeof(data->matched_pattern) - 1] = '\0';
            ci_debug_printf(3, "polis_dlp: Blocked pattern '%s' - "
                               "host '%s' not in allow list\n",
                           patterns[i].name, data->host);
            return 1;
        }

        /* No allow_domain set and not always_block - block by default */
        data->blocked = 1;
        strncpy(data->matched_pattern, patterns[i].name,
                sizeof(data->matched_pattern) - 1);
        data->matched_pattern[
            sizeof(data->matched_pattern) - 1] = '\0';
        ci_debug_printf(3, "polis_dlp: Blocked pattern '%s' - "
                           "no allow rule configured\n",
                       patterns[i].name);
        return 1;
    }

    /* No actionable matches found - allow the request */
    return 0;
}

/*
 * dlp_check_preview - Handle ICAP preview data.
 *
 * Accumulates the preview chunk into the body memory buffer.
 * Does NOT unlock data - we need to scan the full body before deciding.
 */
int dlp_check_preview(char *preview_data, int preview_data_len,
                      ci_request_t *req)
{
    dlp_req_data_t *data = ci_service_data(req);

    if (!data)
        return CI_MOD_CONTINUE;

    /* Initialize OTT fields (Requirement 1.1) */
    data->ott_rewritten = 0;
    data->ott_body_sent = 0;
    data->request_id[0] = '\0';

    /* No body (e.g., GET requests) — still enforce domain policy.
     * Without this check, bodyless requests to unknown domains bypass
     * apply_security_policy() entirely because dlp_process() is only
     * called when CI_MOD_CONTINUE is returned.
     *
     * We return CI_MOD_CONTINUE so c-ICAP proceeds to call
     * dlp_process() (end_of_data handler), which already has the
     * full blocking logic including apply_security_policy(). For
     * no-body requests, c-ICAP will call dlp_process() immediately
     * since there is no more data to read. */
    if (!ci_req_hasbody(req)) {
        int policy = apply_security_policy(data->host, 0);
        if (policy == 0) {
            ci_debug_printf(5, "polis_dlp: No body, known domain "
                               "'%s' — allowing\n", data->host);
            return CI_MOD_ALLOW204;
        }
        ci_debug_printf(3, "polis_dlp: No body, new domain '%s' "
                           "— deferring to end_of_data handler "
                           "(policy=%d)\n", data->host, policy);
        return CI_MOD_CONTINUE;
    }

    /* Accumulate preview data for scanning */
    if (preview_data && preview_data_len > 0) {
        ci_membuf_write(data->body, preview_data, preview_data_len, 0);
        data->total_body_len += preview_data_len;
        ci_debug_printf(5, "polis_dlp: Preview received %d bytes, "
                           "total so far: %zu\n",
                       preview_data_len, data->total_body_len);
    }

    /* Don't unlock data yet - wait until we've scanned the body */
    return CI_MOD_CONTINUE;
}

/*
 * dlp_process - Process the complete request body for DLP scanning.
 *
 * Called by c-ICAP after all body data has been received. Scans the
 * accumulated body (first 1MB) against all credential patterns. If
 * the body exceeded 1MB, also scans the 10KB tail buffer to prevent
 * trivial padding bypass.
 *
 * After credential matching, applies security level policy via
 * apply_security_policy(). For new domains:
 *   - STRICT: blocks with reason "new_domain_blocked"
 *   - BALANCED: blocks with reason "new_domain_prompt" (HITL)
 *   - RELAXED: allows through
 *
 * If blocked (credential or policy):
 *   - Returns HTTP 403 with X-polis diagnostic headers
 *   - Logs the pattern/reason name (never the credential value)
 *
 * If no block triggered:
 *   - Returns 204 (no modification)
 *
 * Requirements: 2.1, 2.2, 2.3
 */
int dlp_process(ci_request_t *req)
{
    dlp_req_data_t *data = ci_service_data(req);
    char hdr_buf[256];

    if (!data || !data->body) {
        if (data)
            data->eof = 1;
        return CI_MOD_DONE;
    }

    /* Null-terminate the body membuf for regex scanning */
    ci_membuf_write(data->body, "\0", 1, 1);

    /* Scan the first 1MB of the body */
    check_patterns(ci_membuf_raw(data->body),
                   (int)data->total_body_len, data);

    /* If body exceeded 1MB, also scan the tail buffer */
    if (data->total_body_len > MAX_BODY_SCAN && data->tail_len > 0) {
        ci_debug_printf(3, "polis_dlp: DLP_PARTIAL_SCAN - "
                           "body size %zu exceeds %d, "
                           "scanning tail buffer (%zu bytes)\n",
                       data->total_body_len, MAX_BODY_SCAN,
                       data->tail_len);

        /* The tail buffer may contain embedded null bytes (e.g., from
         * zero-padded payloads). regexec stops at the first null, so
         * we scan each non-null segment independently. */
        {
            size_t pos = 0;
            while (pos < data->tail_len && !data->blocked) {
                /* Skip null bytes */
                while (pos < data->tail_len && data->tail[pos] == '\0')
                    pos++;
                if (pos >= data->tail_len)
                    break;
                /* Find the end of this non-null segment */
                size_t seg_start = pos;
                while (pos < data->tail_len && data->tail[pos] != '\0')
                    pos++;
                /* Null-terminate the segment (pos is either at a null
                 * byte or at tail_len where we can safely write) */
                if (pos < TAIL_SCAN_SIZE)
                    data->tail[pos] = '\0';
                check_patterns(data->tail + seg_start,
                               (int)(pos - seg_start), data);
            }
        }
    }

    /* Apply security level policy after credential matching.
     * data->blocked from credential matching is passed as
     * has_credential — if already blocked, policy returns 1
     * (prompt) which is already handled above. We only act
     * on the policy result if not already blocked.
     * Requirements: 2.1, 2.2, 2.3 */
    {
        int policy = apply_security_policy(data->host,
                                           data->blocked);
        if (policy == 2 && data->blocked != 1) {
            /* STRICT: block new domain */
            data->blocked = 1;
            strncpy(data->matched_pattern, "new_domain_blocked",
                    sizeof(data->matched_pattern) - 1);
            data->matched_pattern[
                sizeof(data->matched_pattern) - 1] = '\0';
            ci_debug_printf(3, "polis_dlp: BLOCKED new domain "
                               "'%s' — security level STRICT\n",
                           data->host);
        } else if (policy == 1 && data->blocked != 1) {
            /* BALANCED: trigger HITL prompt for new domain */
            data->blocked = 1;
            strncpy(data->matched_pattern, "new_domain_prompt",
                    sizeof(data->matched_pattern) - 1);
            data->matched_pattern[
                sizeof(data->matched_pattern) - 1] = '\0';
            ci_debug_printf(3, "polis_dlp: PROMPT new domain "
                               "'%s' — security level BALANCED\n",
                           data->host);
        }
    }

    /* If blocked, check if destination has a recent host-based approval.
     * This allows retries to pass through after the user approved the
     * original blocked request via the OTT approval flow. */
    if (data->blocked == 1 && data->host[0] != '\0') {
        if (ensure_gov_valkey_connected()) {
            char host_key[320];
            snprintf(host_key, sizeof(host_key),
                     "polis:approved:host:%s", data->host);

            pthread_mutex_lock(&gov_valkey_mutex);
            redisReply *approved_reply = redisCommand(
                valkey_gov_ctx, "EXISTS %s", host_key);

            if (approved_reply &&
                approved_reply->type == REDIS_REPLY_INTEGER &&
                approved_reply->integer == 1) {
                /* Host has been recently approved — allow through */
                ci_debug_printf(3, "polis_dlp: "
                    "Host '%s' has active approval — "
                    "allowing blocked request through\n",
                    data->host);
                data->blocked = 0;
                data->matched_pattern[0] = '\0';
            }
            if (approved_reply) freeReplyObject(approved_reply);
            pthread_mutex_unlock(&gov_valkey_mutex);
        }
    }

    /* If blocked, create 403 response with body (like srv_url_check) */
    if (data->blocked == 1) {
        char body_buf[512];
        int body_len;
        char len_hdr[64];

        /* Generate request_id for this block (req-[a-f0-9]{8}) */
        {
            FILE *fp = fopen("/dev/urandom", "rb");
            if (fp) {
                unsigned char rand_bytes[4];
                if (fread(rand_bytes, 1, 4, fp) == 4) {
                    snprintf(data->request_id, sizeof(data->request_id),
                            "req-%02x%02x%02x%02x",
                            rand_bytes[0], rand_bytes[1],
                            rand_bytes[2], rand_bytes[3]);
                }
                fclose(fp);
            }
        }

        /* Build minimal HTML error page body */
        body_len = snprintf(body_buf, sizeof(body_buf),
            "<html><head><title>403 Forbidden</title></head>"
            "<body><h1>403 Forbidden</h1>"
            "<p>Request blocked by DLP: %s</p></body></html>",
            data->matched_pattern);

        /* Store error page for streaming via dlp_io */
        data->error_page = ci_membuf_new_sized(body_len + 1);
        ci_membuf_write(data->error_page, body_buf, body_len, 1);

        /* Create HTTP response with body (has_reshdr=1, has_body=1) */
        ci_http_response_create(req, 1, 1);
        ci_http_response_add_header(req, "HTTP/1.1 403 Forbidden");
        ci_http_response_add_header(req, "Server: C-ICAP/polis-dlp");
        ci_http_response_add_header(req, "Content-Type: text/html");
        ci_http_response_add_header(req, "Connection: close");

        snprintf(len_hdr, sizeof(len_hdr), "Content-Length: %d", body_len);
        ci_http_response_add_header(req, len_hdr);

        /* Add diagnostic headers */
        ci_http_response_add_header(req, "X-polis-Block: true");

        snprintf(hdr_buf, sizeof(hdr_buf),
                 "X-polis-Reason: %s", data->matched_pattern);
        ci_http_response_add_header(req, hdr_buf);

        snprintf(hdr_buf, sizeof(hdr_buf),
                 "X-polis-Pattern: %s", data->matched_pattern);
        ci_http_response_add_header(req, hdr_buf);

        /* Add request ID header for approval workflow */
        if (data->request_id[0] != '\0') {
            snprintf(hdr_buf, sizeof(hdr_buf),
                     "X-polis-Request-Id: %s", data->request_id);
            ci_http_response_add_header(req, hdr_buf);
        }

        ci_debug_printf(3, "polis_dlp: BLOCKED request to "
                           "'%s' - pattern '%s' matched\n",
                       data->host, data->matched_pattern);

        data->eof = 1;
        ci_req_unlock_data(req);
        return CI_MOD_DONE;
    }

    /* --- OTT rewrite pass (Requirements 1.3-1.7, 1.10) --- */
    /* Only scan for approve pattern if body passed DLP + security policy */
    {
        regmatch_t matches[2];
        char *body_raw = (char *)ci_membuf_raw(data->body);

        if (body_raw &&
            regexec(&approve_pattern, body_raw, 2, matches, 0) == 0) {
            /* Extract request_id from match group 1 */
            int req_id_len = matches[1].rm_eo - matches[1].rm_so;
            char request_id[32];
            
            if (req_id_len > 0 && req_id_len < (int)sizeof(request_id)) {
                memcpy(request_id, body_raw + matches[1].rm_so, req_id_len);
                request_id[req_id_len] = '\0';
                
                ci_debug_printf(3, "polis_dlp: Found approve pattern with "
                               "request_id='%s'\n", request_id);
                
                /* Validate request_id format: req-[a-f0-9]{8} (CWE-116) */
                if (req_id_len == 12 &&
                    request_id[0] == 'r' && request_id[1] == 'e' &&
                    request_id[2] == 'q' && request_id[3] == '-') {
                    int valid = 1;
                    int i;
                    for (i = 4; i < 12; i++) {
                        char c = request_id[i];
                        if (!((c >= '0' && c <= '9') ||
                              (c >= 'a' && c <= 'f'))) {
                            valid = 0;
                            break;
                        }
                    }
                    
                    if (valid) {
                        /* Check Host header present (context binding) */
                        if (data->host[0] == '\0') {
                            ci_debug_printf(1, "polis_dlp: WARNING: "
                                           "approve pattern found but no Host "
                                           "header — skipping OTT rewrite\n");
                        } else if (!ensure_gov_valkey_connected()) {
                            /* Fail-closed: block if Valkey unavailable (H3) */
                            ci_debug_printf(0, "CRITICAL: polis_dlp: "
                                           "governance-reqmod Valkey down, "
                                           "blocking /polis-approve to prevent "
                                           "request_id leak (CWE-209)\n");
                            
                            /* Return 403 with retry message */
                            ci_http_response_create(req, 1, 1);
                            ci_http_response_add_header(req,
                                "HTTP/1.1 403 Forbidden");
                            ci_http_response_add_header(req,
                                "X-polis-Block: approval_service_unavailable");
                            ci_http_response_add_header(req,
                                "Content-Type: text/plain");
                            
                            const char *err_msg = "Approval service temporarily "
                                "unavailable. Please retry in a moment.\n";
                            data->error_page = ci_membuf_new_sized(strlen(err_msg) + 1);
                            ci_membuf_write(data->error_page, err_msg,
                                          strlen(err_msg), 1);
                            
                            snprintf(hdr_buf, sizeof(hdr_buf),
                                    "Content-Length: %zu", strlen(err_msg));
                            ci_http_response_add_header(req, hdr_buf);
                            
                            data->blocked = 1;
                            data->eof = 1;
                            ci_req_unlock_data(req);
                            return CI_MOD_DONE;
                        } else {
                            /* Proceed with OTT rewrite */
                            ci_debug_printf(3, "polis_dlp: "
                                           "Validated request_id format\n");
                            
                            /* Acquire OTT lock (H5: TOCTOU prevention) */
                            pthread_mutex_lock(&gov_valkey_mutex);
                            redisReply *lock_reply = redisCommand(valkey_gov_ctx,
                                "SET polis:ott_lock:%s 1 NX EX 30", request_id);
                            
                            if (!lock_reply || lock_reply->type == REDIS_REPLY_NIL) {
                                /* Lock contention — another thread processing */
                                ci_debug_printf(2, "polis_dlp: OTT lock "
                                               "contention for %s, skipping\n",
                                               request_id);
                                if (lock_reply) freeReplyObject(lock_reply);
                                pthread_mutex_unlock(&gov_valkey_mutex);
                            } else {
                                freeReplyObject(lock_reply);
                                
                                /* Check blocked key exists */
                                redisReply *blocked_reply = redisCommand(
                                    valkey_gov_ctx,
                                    "EXISTS polis:blocked:%s", request_id);
                                
                                if (!blocked_reply ||
                                    blocked_reply->type != REDIS_REPLY_INTEGER ||
                                    blocked_reply->integer != 1) {
                                    ci_debug_printf(2, "polis_dlp: "
                                                   "blocked key does not exist "
                                                   "for %s — skipping\n",
                                                   request_id);
                                    if (blocked_reply) freeReplyObject(blocked_reply);
                                    pthread_mutex_unlock(&gov_valkey_mutex);
                                } else {
                                    freeReplyObject(blocked_reply);
                                    
                                    /* Generate OTT code */
                                    char ott_code[OTT_LEN + 1];
                                    if (generate_ott(ott_code, sizeof(ott_code)) != 0) {
                                        ci_debug_printf(0, "CRITICAL: polis_dlp: "
                                                       "OTT generation failed — "
                                                       "skipping rewrite\n");
                                        pthread_mutex_unlock(&gov_valkey_mutex);
                                    } else {
                                        /* Store OTT mapping in Valkey */
                                        ci_debug_printf(3, "polis_dlp: "
                                                       "Generated OTT: %s\n",
                                                       ott_code);
                                        
                                        /* Build JSON payload */
                                        time_t now = time(NULL);
                                        time_t armed_after = now + time_gate_secs;
                                        char json_payload[512];
                                        snprintf(json_payload, sizeof(json_payload),
                                                "{\"ott_code\":\"%s\","
                                                "\"request_id\":\"%s\","
                                                "\"armed_after\":%ld,"
                                                "\"origin_host\":\"%s\"}",
                                                ott_code, request_id,
                                                (long)armed_after, data->host);
                                        
                                        /* Store with SET NX EX */
                                        redisReply *set_reply = redisCommand(
                                            valkey_gov_ctx,
                                            "SET polis:ott:%s %s NX EX %d",
                                            ott_code, json_payload, ott_ttl_secs);
                                        
                                        if (!set_reply ||
                                            set_reply->type == REDIS_REPLY_NIL) {
                                            /* OTT collision — retry once */
                                            ci_debug_printf(1, "polis_dlp: "
                                                           "OTT collision, "
                                                           "retrying\n");
                                            if (set_reply) freeReplyObject(set_reply);
                                            
                                            if (generate_ott(ott_code,
                                                           sizeof(ott_code)) != 0) {
                                                ci_debug_printf(0, "CRITICAL: "
                                                               "OTT retry failed\n");
                                                pthread_mutex_unlock(&gov_valkey_mutex);
                                            } else {
                                                /* Retry SET with new OTT */
                                                snprintf(json_payload,
                                                        sizeof(json_payload),
                                                        "{\"ott_code\":\"%s\","
                                                        "\"request_id\":\"%s\","
                                                        "\"armed_after\":%ld,"
                                                        "\"origin_host\":\"%s\"}",
                                                        ott_code, request_id,
                                                        (long)armed_after,
                                                        data->host);
                                                
                                                set_reply = redisCommand(
                                                    valkey_gov_ctx,
                                                    "SET polis:ott:%s %s NX EX %d",
                                                    ott_code, json_payload,
                                                    ott_ttl_secs);
                                                
                                                if (!set_reply ||
                                                    set_reply->type ==
                                                    REDIS_REPLY_NIL) {
                                                    ci_debug_printf(0,
                                                        "CRITICAL: OTT retry "
                                                        "collision — fail closed\n");
                                                    if (set_reply)
                                                        freeReplyObject(set_reply);
                                                    pthread_mutex_unlock(
                                                        &gov_valkey_mutex);
                                                } else {
                                                    /* Retry succeeded (continue) */
                                                    freeReplyObject(set_reply);
                                                }
                                            }
                                        } else {
                                            freeReplyObject(set_reply);
                                            /* OTT stored successfully */
                                            
                                            /* Perform length-preserving substitution */
                                            /* Replace request_id with ott_code in membuf */
                                            int sub_offset = matches[1].rm_so;
                                            
                                            /* Verify both are same length (12 chars) */
                                            if (req_id_len == OTT_LEN) {
                                                memcpy(body_raw + sub_offset,
                                                      ott_code, OTT_LEN);
                                                
                                                /* Verify size match (H6) */
                                                size_t modified_size =
                                                    ci_membuf_size(data->body);
                                                if (modified_size !=
                                                    data->total_body_len + 1) {
                                                    /* +1 for null terminator */
                                                    ci_debug_printf(0,
                                                        "polis_dlp: OTT "
                                                        "substitution size "
                                                        "mismatch: original=%zu "
                                                        "modified=%zu — "
                                                        "falling back\n",
                                                        data->total_body_len + 1,
                                                        modified_size);
                                                    data->ott_rewritten = 0;
                                                } else {
                                                    data->ott_rewritten = 1;
                                                    data->ott_body_sent = 0;
                                                    
                                                    ci_debug_printf(3,
                                                        "polis_dlp: OTT rewrite "
                                                        "complete: %s -> %s\n",
                                                        request_id, ott_code);
                                                    
                                                    /* Log to audit (H8) */
                                                    char audit_json[512];
                                                    snprintf(audit_json,
                                                            sizeof(audit_json),
                                                            "{\"event\":\"ott_rewrite\","
                                                            "\"request_id\":\"%s\","
                                                            "\"ott_code\":\"%s\","
                                                            "\"origin_host\":\"%s\","
                                                            "\"timestamp\":%ld}",
                                                            request_id, ott_code,
                                                            data->host, (long)now);
                                                    
                                                    redisReply *log_reply =
                                                        redisCommand(valkey_gov_ctx,
                                                            "ZADD polis:log:events "
                                                            "%ld %s",
                                                            (long)now, audit_json);
                                                    if (log_reply)
                                                        freeReplyObject(log_reply);
                                                }
                                            } else {
                                                ci_debug_printf(0, "CRITICAL: "
                                                    "polis_dlp: request_id length "
                                                    "mismatch (%d != %d)\n",
                                                    req_id_len, OTT_LEN);
                                            }
                                            
                                            pthread_mutex_unlock(&gov_valkey_mutex);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        ci_debug_printf(1, "polis_dlp: WARNING: "
                                       "request_id format invalid "
                                       "(non-hex chars) — skipping\n");
                    }
                } else {
                    ci_debug_printf(1, "polis_dlp: WARNING: "
                                   "request_id length/prefix invalid — "
                                   "skipping\n");
                }
            }
        }
    }

    /* No credential detected or allowed - pass through unchanged */
    data->eof = 1;
    ci_req_unlock_data(req);
    return CI_MOD_DONE;
}

/*
 * dlp_io - Handle body data streaming during REQMOD.
 *
 * Accumulates body data for scanning. Only streams data back AFTER
 * dlp_process() has made the block/allow decision (eof is set).
 * When blocked, streams the error page body instead.
 * Returns CI_OK on success.
 */
int dlp_io(char *wbuf, int *wlen, char *rbuf, int *rlen,
           int iseof, ci_request_t *req)
{
    dlp_req_data_t *data = ci_service_data(req);
    int ret = CI_OK;

    /* Accumulate incoming body data for scanning */
    if (rlen && rbuf && *rlen > 0 && data) {
        /* Accumulate for scanning (up to MAX_BODY_SCAN) */
        if (data->body && data->total_body_len < MAX_BODY_SCAN) {
            int space = MAX_BODY_SCAN - (int)data->total_body_len;
            int to_write = (*rlen < space) ? *rlen : space;
            ci_membuf_write(data->body, rbuf, to_write, 0);
        }

        /* Maintain rolling tail buffer (last TAIL_SCAN_SIZE bytes)
         * for detecting credentials appended after the 1MB scan window */
        {
            int chunk = *rlen;
            if (chunk >= TAIL_SCAN_SIZE) {
                /* Chunk alone fills the tail — just keep the last TAIL_SCAN_SIZE bytes */
                memcpy(data->tail, rbuf + chunk - TAIL_SCAN_SIZE, TAIL_SCAN_SIZE);
                data->tail_len = TAIL_SCAN_SIZE;
            } else if (data->tail_len + (size_t)chunk <= TAIL_SCAN_SIZE) {
                /* Fits without eviction */
                memcpy(data->tail + data->tail_len, rbuf, chunk);
                data->tail_len += chunk;
            } else {
                /* Shift old data left to make room */
                size_t keep = TAIL_SCAN_SIZE - chunk;
                memmove(data->tail, data->tail + data->tail_len - keep, keep);
                memcpy(data->tail + keep, rbuf, chunk);
                data->tail_len = TAIL_SCAN_SIZE;
            }
        }

        /* Also write to cached file for later pass-through if allowed */
        if (data->ring) {
            if (ci_cached_file_write(data->ring, rbuf, *rlen, iseof) < 0)
                ret = CI_ERROR;
        }
        data->total_body_len += *rlen;
    }

    /* Only send data back AFTER dlp_process() has run (eof is set) */
    if (wbuf && wlen) {
        if (!data || !data->eof) {
            /* Not ready to send yet - still accumulating */
            *wlen = 0;
        } else if (data->blocked && data->error_page) {
            /* Stream error page body for blocked response */
            int avail = ci_membuf_size(data->error_page) - data->error_page_sent;
            if (avail > 0) {
                int to_send = (avail < *wlen) ? avail : *wlen;
                memcpy(wbuf, ci_membuf_raw(data->error_page) + data->error_page_sent, to_send);
                data->error_page_sent += to_send;
                *wlen = to_send;
            } else {
                *wlen = CI_EOF;
            }
        } else if (data->ott_rewritten && data->body) {
            /* Stream from modified membuf (OTT-rewritten body) */
            /* The membuf contains the body with req-id replaced by OTT.
             * We can't use the cached file because it has the original
             * unmodified body. Note: membuf has null terminator, so
             * size is total_body_len + 1, but we only send total_body_len */
            int avail = (int)data->total_body_len - (int)data->ott_body_sent;
            if (avail > 0) {
                int to_send = (avail < *wlen) ? avail : *wlen;
                memcpy(wbuf, ci_membuf_raw(data->body) + data->ott_body_sent,
                       to_send);
                data->ott_body_sent += to_send;
                *wlen = to_send;
            } else {
                *wlen = CI_EOF;
            }
        } else if (data->ring) {
            /* Normal pass-through from cached file */
            *wlen = ci_cached_file_read(data->ring, wbuf, *wlen);
            if (*wlen == 0)
                *wlen = CI_EOF;
        } else {
            *wlen = CI_EOF;
        }
    }

    return ret;
}
