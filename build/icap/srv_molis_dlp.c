/*
 * srv_molis_dlp.c - c-ICAP DLP module for credential detection
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
    char tail[TAIL_SCAN_SIZE];  /* Last 10KB ring buffer for tail scan */
    size_t tail_len;            /* Bytes currently in tail buffer */
    size_t total_body_len;      /* Total body length seen so far */
    char host[256];             /* Host header value from request */
    int blocked;                /* Whether this request was blocked */
    char matched_pattern[64];   /* Name of the pattern that matched */
} dlp_req_data_t;

/* Static pattern storage - loaded from config at service init */
static dlp_pattern_t patterns[MAX_PATTERNS];
static int pattern_count = 0;

/*
 * Security level enum — maps to Valkey values at molis:config:security_level.
 * Controls DLP behavior for new (unknown) domains.
 */
typedef enum {
    LEVEL_RELAXED  = 0,   /* New domains: auto-allow */
    LEVEL_BALANCED = 1,   /* New domains: HITL prompt (default) */
    LEVEL_STRICT   = 2    /* New domains: block */
} security_level_t;

/* Valkey polling constants */
#define LEVEL_POLL_INTERVAL 100    /* Requests between Valkey polls */
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

/*
 * refresh_security_level - Poll Valkey for the current security level.
 *
 * Executes GET molis:config:security_level via hiredis. On success,
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

    /* No Valkey connection — skip polling entirely */
    if (valkey_level_ctx == NULL)
        return;

    reply = redisCommand(valkey_level_ctx,
                         "GET molis:config:security_level");

    /* Failure path: keep current level, backoff */
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        current_poll_interval *= 2;
        if (current_poll_interval > LEVEL_POLL_MAX)
            current_poll_interval = LEVEL_POLL_MAX;
        ci_debug_printf(1, "molis_dlp: Valkey poll failed, "
                           "keeping level %d, next poll in "
                           "%lu requests\n",
                       (int)current_level,
                       current_poll_interval);
        if (reply)
            freeReplyObject(reply);
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
        ci_debug_printf(1, "molis_dlp: Unknown security level "
                           "'%s', defaulting to balanced\n",
                       val);
        current_level = LEVEL_BALANCED;
    }

    ci_debug_printf(5, "molis_dlp: Security level updated "
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
 * Reads MOLIS_VALKEY_HOST env var (default: "valkey"), port 6379.
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
static int dlp_valkey_init(void)
{
    const char *vk_host;
    int vk_port = 6379;
    redisSSLContext *ssl_ctx = NULL;
    redisSSLContextError ssl_err;
    redisReply *reply;
    FILE *fp;
    char password[256];
    size_t pass_len;

    /* Lock: all Valkey state modifications under mutex */
    pthread_mutex_lock(&valkey_mutex);

    /* Read Valkey host from environment (default: "valkey") */
    vk_host = getenv("MOLIS_VALKEY_HOST");
    if (!vk_host) vk_host = "valkey";

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
        ci_debug_printf(1, "molis_dlp: WARNING: "
            "Failed to create TLS context: %s — "
            "Valkey connection unavailable\n",
            redisSSLContextGetError(ssl_err));
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }

    /* Establish TCP connection to Valkey */
    valkey_level_ctx = redisConnect(vk_host, vk_port);
    if (valkey_level_ctx == NULL ||
        valkey_level_ctx->err) {
        ci_debug_printf(1, "molis_dlp: WARNING: "
            "Cannot connect to Valkey at %s:%d%s%s — "
            "Valkey connection unavailable\n",
            vk_host, vk_port,
            valkey_level_ctx ? ": " : "",
            valkey_level_ctx ? valkey_level_ctx->errstr : "");
        if (valkey_level_ctx) {
            redisFree(valkey_level_ctx);
            valkey_level_ctx = NULL;
        }
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }

    /* Initiate TLS handshake on the connection */
    if (redisInitiateSSLWithContext(valkey_level_ctx,
                                    ssl_ctx) != REDIS_OK) {
        ci_debug_printf(1, "molis_dlp: WARNING: "
            "TLS handshake failed with Valkey: %s — "
            "Valkey connection unavailable\n",
            valkey_level_ctx->errstr);
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }

    /* Read dlp-reader password from Docker secret file */
    fp = fopen("/run/secrets/valkey_dlp_password", "r");
    if (!fp) {
        ci_debug_printf(1, "molis_dlp: WARNING: "
            "Cannot open /run/secrets/valkey_dlp_password — "
            "ACL authentication unavailable\n");
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }

    memset(password, 0, sizeof(password));
    if (fgets(password, sizeof(password), fp) == NULL) {
        ci_debug_printf(1, "molis_dlp: WARNING: "
            "Failed to read password from "
            "/run/secrets/valkey_dlp_password\n");
        fclose(fp);
        memset(password, 0, sizeof(password));
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&valkey_mutex);
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

    /* Authenticate with ACL: AUTH dlp-reader <password> */
    reply = redisCommand(valkey_level_ctx,
        "AUTH dlp-reader %s", password);

    /* Scrub password from stack immediately after AUTH */
    memset(password, 0, sizeof(password));

    if (reply == NULL ||
        reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "molis_dlp: CRITICAL: "
            "Valkey ACL auth failed as dlp-reader%s%s — "
            "Valkey connection unavailable\n",
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        redisFree(valkey_level_ctx);
        valkey_level_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }
    freeReplyObject(reply);

    ci_debug_printf(3, "molis_dlp: "
        "Authenticated as dlp-reader\n");

    ci_debug_printf(3, "molis_dlp: "
        "Connected to Valkey at %s:%d (TLS + ACL)\n",
        vk_host, vk_port);

    redisFreeSSLContext(ssl_ctx);

    /* Read initial security level from Valkey */
    refresh_security_level();

    pthread_mutex_unlock(&valkey_mutex);
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
 * Registers the DLP module as a REQMOD service named "molis_dlp".
 */
CI_DECLARE_MOD_DATA ci_service_module_t service = {
    "molis_dlp",                     /* mod_name */
    "Molis DLP credential detection service",  /* mod_short_descr */
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
 * Parses /etc/c-icap/molis_dlp.conf to load credential patterns,
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

    ci_debug_printf(3, "molis_dlp: Initializing service, "
                       "loading config from /etc/c-icap/molis_dlp.conf\n");

    fp = fopen("/etc/c-icap/molis_dlp.conf", "r");
    if (!fp) {
        ci_debug_printf(0, "molis_dlp: CRITICAL: Cannot open config file "
                           "/etc/c-icap/molis_dlp.conf — refusing to start\n");
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
                ci_debug_printf(1, "molis_dlp: WARNING: Max patterns (%d) "
                                   "reached, skipping '%s'\n",
                                MAX_PATTERNS, name);
                continue;
            }
            if (regcomp(&patterns[pattern_count].regex, value,
                        REG_EXTENDED | REG_NOSUB) != 0) {
                ci_debug_printf(1, "molis_dlp: ERROR: Failed to compile "
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
            ci_debug_printf(3, "molis_dlp: Loaded pattern '%s'\n", name);
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
                    ci_debug_printf(3, "molis_dlp: Set allow domain for "
                                       "'%s': %s\n", name, value);
                } else {
                    pat->allow_compiled = 0;
                    ci_debug_printf(1, "molis_dlp: ERROR: Failed to compile "
                                       "allow regex for '%s'\n", name);
                }
            } else {
                ci_debug_printf(1, "molis_dlp: WARNING: Allow rule for "
                                   "unknown pattern '%s'\n", name);
            }
            continue;
        }

        /* Parse action.<name> = block */
        if (sscanf(line, " action.%63[^ ] = %255[^\n]", name, value) == 2) {
            pat = find_pattern_by_name(name);
            if (pat && strcmp(value, "block") == 0) {
                pat->always_block = 1;
                ci_debug_printf(3, "molis_dlp: Set always_block for "
                                   "'%s'\n", name);
            } else if (!pat) {
                ci_debug_printf(1, "molis_dlp: WARNING: Action for "
                                   "unknown pattern '%s'\n", name);
            }
            continue;
        }
    }

    fclose(fp);

    ci_debug_printf(3, "molis_dlp: Initialization complete, "
                       "%d patterns loaded\n", pattern_count);

    /* Fail-closed: refuse to start if no credential patterns loaded (CWE-636) */
    if (pattern_count == 0) {
        ci_debug_printf(0, "molis_dlp: CRITICAL: No credential patterns "
                           "loaded from molis_dlp.conf — refusing to start "
                           "(fail-closed, CWE-636)\n");
        return CI_ERROR;
    }

    /* Initialize Valkey connection for dynamic security levels (non-fatal) */
    if (dlp_valkey_init() != 0) {
        ci_debug_printf(2, "molis_dlp: WARNING: Valkey init failed — "
                           "DLP will operate without dynamic security "
                           "levels, defaulting to balanced\n");
    }

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
    ci_debug_printf(3, "molis_dlp: Closing service, "
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
        ci_debug_printf(1, "molis_dlp: ERROR: Failed to allocate "
                           "request data\n");
        return NULL;
    }

    /* Create memory buffer for body accumulation (up to 1MB) */
    data->body = ci_membuf_new_sized(MAX_BODY_SCAN);
    data->tail_len = 0;
    data->total_body_len = 0;
    data->host[0] = '\0';
    data->blocked = 0;
    data->matched_pattern[0] = '\0';
    memset(data->tail, 0, TAIL_SCAN_SIZE);

    /* Extract Host header from the HTTP request */
    host_hdr = ci_http_request_get_header(req, "Host");
    if (host_hdr) {
        strncpy(data->host, host_hdr, sizeof(data->host) - 1);
        data->host[sizeof(data->host) - 1] = '\0';
        ci_debug_printf(5, "molis_dlp: Request to host: %s\n",
                        data->host);
    } else {
        ci_debug_printf(5, "molis_dlp: No Host header found\n");
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
        ci_debug_printf(3, "molis_dlp: Pattern '%s' matched\n",
                        patterns[i].name);

        /* Always-block patterns (e.g., private keys) */
        if (patterns[i].always_block) {
            data->blocked = 1;
            strncpy(data->matched_pattern, patterns[i].name,
                    sizeof(data->matched_pattern) - 1);
            data->matched_pattern[
                sizeof(data->matched_pattern) - 1] = '\0';
            ci_debug_printf(3, "molis_dlp: Blocked by always_block "
                               "pattern '%s'\n", patterns[i].name);
            return 1;
        }

        /* Pattern has a pre-compiled allow_domain - check host against it */
        if (patterns[i].allow_compiled) {
            if (regexec(&patterns[i].allow_regex, data->host,
                        0, NULL, 0) == 0) {
                /* Host matches allow rule - credential going to
                   expected destination, continue scanning */
                ci_debug_printf(3, "molis_dlp: Pattern '%s' "
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
            ci_debug_printf(3, "molis_dlp: Blocked pattern '%s' - "
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
        ci_debug_printf(3, "molis_dlp: Blocked pattern '%s' - "
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
 * Accumulates the preview chunk into the body memory buffer
 * and updates the total body length counter. Returns
 * CI_MOD_CONTINUE to request the full request body.
 */
int dlp_check_preview(char *preview_data, int preview_data_len,
                      ci_request_t *req)
{
    dlp_req_data_t *data = ci_service_data(req);

    if (!data || !preview_data || preview_data_len <= 0)
        return CI_MOD_CONTINUE;

    ci_membuf_write(data->body, preview_data, preview_data_len, 0);
    data->total_body_len += preview_data_len;

    ci_debug_printf(5, "molis_dlp: Preview received %d bytes, "
                       "total so far: %zu\n",
                   preview_data_len, data->total_body_len);

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
 *   - Returns HTTP 403 with X-Molis diagnostic headers
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

    if (!data || !data->body)
        return CI_MOD_ALLOW204;

    /* Null-terminate the body membuf for regex scanning */
    ci_membuf_write(data->body, "\0", 1, 1);

    /* Scan the first 1MB of the body */
    check_patterns(ci_membuf_raw(data->body),
                   (int)data->total_body_len, data);

    /* If body exceeded 1MB, also scan the tail buffer */
    if (data->total_body_len > MAX_BODY_SCAN) {
        ci_debug_printf(3, "molis_dlp: DLP_PARTIAL_SCAN - "
                           "body size %zu exceeds %d, "
                           "scanning tail buffer\n",
                       data->total_body_len, MAX_BODY_SCAN);

        /* Null-terminate the tail buffer */
        if (data->tail_len < TAIL_SCAN_SIZE) {
            data->tail[data->tail_len] = '\0';
        } else {
            data->tail[TAIL_SCAN_SIZE - 1] = '\0';
        }

        check_patterns(data->tail, (int)data->tail_len, data);
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
            ci_debug_printf(3, "molis_dlp: BLOCKED new domain "
                               "'%s' — security level STRICT\n",
                           data->host);
        } else if (policy == 1 && data->blocked != 1) {
            /* BALANCED: trigger HITL prompt for new domain */
            data->blocked = 1;
            strncpy(data->matched_pattern, "new_domain_prompt",
                    sizeof(data->matched_pattern) - 1);
            data->matched_pattern[
                sizeof(data->matched_pattern) - 1] = '\0';
            ci_debug_printf(3, "molis_dlp: PROMPT new domain "
                               "'%s' — security level BALANCED\n",
                           data->host);
        }
    }

    /* If blocked, create 403 response with X-Molis headers */
    if (data->blocked == 1) {
        ci_http_response_create(req, 1, 1);
        ci_http_response_add_header(req,
            "HTTP/1.1 403 Forbidden");
        ci_http_response_add_header(req,
            "X-Molis-Block: true");

        snprintf(hdr_buf, sizeof(hdr_buf),
                 "X-Molis-Reason: %s", data->matched_pattern);
        ci_http_response_add_header(req, hdr_buf);

        snprintf(hdr_buf, sizeof(hdr_buf),
                 "X-Molis-Pattern: %s", data->matched_pattern);
        ci_http_response_add_header(req, hdr_buf);

        ci_debug_printf(3, "molis_dlp: BLOCKED request to "
                           "'%s' - pattern '%s' matched\n",
                       data->host, data->matched_pattern);

        return CI_MOD_DONE;
    }

    /* No credential detected or allowed - pass through */
    return CI_MOD_ALLOW204;
}

/*
 * dlp_io - Handle body data streaming during REQMOD.
 *
 * Accumulates request body data:
 *   - First MAX_BODY_SCAN (1MB) bytes go into the ci_membuf.
 *   - Bytes beyond 1MB are written into a rolling 10KB tail buffer
 *     so the last 10KB of the body is always available for scanning.
 *   - We never modify the request body, so wlen is set to 0.
 *
 * Returns CI_OK on success.
 */
int dlp_io(char *wbuf, int *wlen, char *rbuf, int *rlen,
           int iseof, ci_request_t *req)
{
    dlp_req_data_t *data = ci_service_data(req);
    int bytes_to_read;
    int membuf_space;
    int membuf_write;
    int tail_bytes;
    int tail_offset;

    (void)iseof;

    /* We don't modify the request body - pass through unchanged */
    if (wbuf && wlen)
        *wlen = 0;

    if (!data || !rbuf || !rlen || *rlen <= 0)
        return CI_OK;

    bytes_to_read = *rlen;

    /* Case 1: All incoming bytes fit within the 1MB membuf limit */
    if (data->total_body_len + bytes_to_read <= MAX_BODY_SCAN) {
        ci_membuf_write(data->body, rbuf, bytes_to_read, 0);
        data->total_body_len += bytes_to_read;
        return CI_OK;
    }

    /* Case 2: Some bytes go to membuf, rest to tail buffer */
    if (data->total_body_len < MAX_BODY_SCAN) {
        membuf_space = MAX_BODY_SCAN - (int)data->total_body_len;
        membuf_write = (bytes_to_read < membuf_space)
                       ? bytes_to_read : membuf_space;
        ci_membuf_write(data->body, rbuf, membuf_write, 0);
        rbuf += membuf_write;
        bytes_to_read -= membuf_write;
        data->total_body_len += membuf_write;
    }

    /* Remaining bytes go into the rolling tail buffer */
    if (bytes_to_read > 0) {
        data->total_body_len += bytes_to_read;

        if (bytes_to_read >= TAIL_SCAN_SIZE) {
            /* Incoming chunk is larger than tail buffer -
               keep only the last TAIL_SCAN_SIZE bytes */
            tail_offset = bytes_to_read - TAIL_SCAN_SIZE;
            memcpy(data->tail, rbuf + tail_offset, TAIL_SCAN_SIZE);
            data->tail_len = TAIL_SCAN_SIZE;
        } else {
            /* Incoming chunk fits - append with wrap-around */
            tail_bytes = bytes_to_read;
            if (data->tail_len + tail_bytes <= TAIL_SCAN_SIZE) {
                /* Fits without wrapping */
                memcpy(data->tail + data->tail_len,
                       rbuf, tail_bytes);
                data->tail_len += tail_bytes;
            } else {
                /* Need to shift: drop oldest bytes to make room */
                int new_len = data->tail_len + tail_bytes;
                int drop = new_len - TAIL_SCAN_SIZE;
                if (drop > 0 && drop < (int)data->tail_len) {
                    memmove(data->tail,
                            data->tail + drop,
                            data->tail_len - drop);
                    data->tail_len -= drop;
                } else if (drop >= (int)data->tail_len) {
                    data->tail_len = 0;
                }
                memcpy(data->tail + data->tail_len,
                       rbuf, tail_bytes);
                data->tail_len += tail_bytes;
            }
        }
    }

    return CI_OK;
}
