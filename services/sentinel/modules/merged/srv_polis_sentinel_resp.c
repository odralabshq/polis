/*
 * srv_polis_sentinel_resp.c - c-ICAP unified RESPMOD module
 *
 * RESPMOD service combining ClamAV virus scanning with OTT approval detection.
 * Replaces squidclamav with direct clamd INSTREAM protocol implementation.
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
#include <time.h>
#include <unistd.h>
#include <errno.h>

/* Network headers */
#include <sys/socket.h>
#include <sys/un.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <netdb.h>

/* Valkey/Redis client */
#include <hiredis/hiredis.h>
#include <hiredis/hiredis_ssl.h>

/* zlib for gzip decompression */
#include <zlib.h>

/* Constants */
#define CLAMD_CHUNK_SIZE    16384   /* 16KB chunks (matches squidclamav) */
#define CLAMD_TIMEOUT_SECS  30      /* Socket read/write timeout */
#define CLAMD_MAX_RESPONSE  1024    /* Max response line length */
#define MAX_BODY_SIZE       (2 * 1024 * 1024)  /* 2MB body accumulation limit */
#define APPROVAL_TTL_SECS   300     /* Approval key TTL: 5 minutes */

/* Forward declarations - service callbacks */
static int sentinel_resp_init_service(ci_service_xdata_t *srv_xdata, struct ci_server_conf *server_conf);
static void sentinel_resp_close_service(void);
static void *sentinel_resp_init_request_data(ci_request_t *req);
static void sentinel_resp_release_request_data(void *data);
static int sentinel_resp_check_preview(char *preview_data, int preview_data_len, ci_request_t *req);
static int sentinel_resp_process(ci_request_t *req);
static int sentinel_resp_io(char *wbuf, int *wlen, char *rbuf, int *rlen,
                           int iseof, ci_request_t *req);

/* Forward declarations - helper functions */
static int clamd_scan_buffer(const char *buf, size_t len,
                             char *result, size_t result_len);
static int is_allowed_domain(const char *host);
static int process_ott_approval(const char *ott_code, const char *resp_host);
static int valkey_init(void);
static int ensure_valkey_connected(void);
static int decompress_gzip(const char *in, size_t in_len,
                          char **out, size_t *out_len);
static int compress_gzip(const char *in, size_t in_len,
                        char **out, size_t *out_len);
static int clamd_cb_allow_request(void);
static void clamd_cb_record_success(void);
static void clamd_cb_record_failure(void);

/* --- Per-request data structure --- */
typedef struct {
    /* Body accumulation */
    ci_membuf_t      *body;             /* Accumulated response body */
    ci_cached_file_t *cached;           /* Cached file for pass-through */
    size_t            total_body_len;   /* Total body length */
    char              host[256];        /* Response Host header */
    int               is_gzip;          /* Content-Encoding is gzip */
    int               eof;              /* End of data received */

    /* ClamAV scan state */
    int               virus_found;      /* ClamAV detected a virus */
    char              virus_name[256];  /* Virus name from clamd response */

    /* OTT scan state */
    int               ott_found;        /* OTT code was found and processed */
    ci_membuf_t      *error_page;       /* Error page for virus block */
    size_t            error_page_sent;  /* Error page bytes sent */
} sentinel_resp_data_t;

/* --- Static state --- */
static regex_t ott_regex;                    /* OTT pattern: ott-[a-zA-Z0-9]{8} */
static char *allowed_domains[32];            /* Domain allowlist (dot-prefixed) */
static int allowed_domains_count = 0;        /* Number of domains in allowlist */
static char clamd_socket_path[256] = "/var/run/clamav/clamd.sock";  /* clamd Unix socket */
static char clamd_host[256] = "scanner";     /* clamd TCP host (default: scanner) */
static int  clamd_port = 3310;               /* clamd TCP port */
static int  clamd_use_tcp = 1;               /* 1=TCP (default), 0=Unix socket */
static redisContext *valkey_ctx = NULL;      /* governance-respmod connection */
static pthread_mutex_t valkey_mutex = PTHREAD_MUTEX_INITIALIZER;

/* Circuit breaker for clamd */
#define CB_FAILURE_THRESHOLD  5    /* Open after 5 failures */
#define CB_RECOVERY_SECS      30   /* Try again after 30s */

typedef struct {
    int failure_count;
    time_t last_failure;
    enum { CB_CLOSED, CB_OPEN, CB_HALF_OPEN } state;
    pthread_mutex_t mutex;
} circuit_breaker_t;

static circuit_breaker_t clamd_cb = {
    .failure_count = 0,
    .last_failure = 0,
    .state = CB_CLOSED,
    .mutex = PTHREAD_MUTEX_INITIALIZER
};


/* --- Service registration --- */
CI_DECLARE_MOD_DATA ci_service_module_t service = {
    "polis_sentinel_resp",                       /* mod_name */
    "polis sentinel ClamAV + approval (RESPMOD)",/* mod_short_descr */
    ICAP_RESPMOD,                                /* mod_type */
    sentinel_resp_init_service,                  /* mod_init_service */
    NULL,                                        /* mod_post_init_service */
    sentinel_resp_close_service,                 /* mod_close_service */
    sentinel_resp_init_request_data,             /* mod_init_request_data */
    sentinel_resp_release_request_data,          /* mod_release_request_data */
    sentinel_resp_check_preview,                 /* mod_check_preview_handler */
    sentinel_resp_process,                       /* mod_end_of_data_handler */
    sentinel_resp_io,                            /* mod_service_io */
    NULL,                                        /* mod_conf_table */
    NULL                                         /* mod_data */
};

/* ========================================================================== */
/* Service Lifecycle Callbacks                                                */
/* ========================================================================== */

/**
 * sentinel_resp_init_service() - Initialize the RESPMOD service
 *
 * Called once at c-ICAP startup. Performs:
 * 1. Compile OTT regex pattern
 * 2. Load domain allowlist from environment
 * 3. Load clamd socket path from environment
 * 4. Connect to Valkey as governance-respmod via TLS
 * 5. Initialize circuit breaker mutex
 *
 * Returns: CI_OK on success, CI_ERROR on failure
 */
static int sentinel_resp_init_service(ci_service_xdata_t *srv_xdata, struct ci_server_conf *server_conf)
{
    int ret;

    ci_debug_printf(2, "sentinel_resp: Initializing service\n");

    /* Enable ICAP 204 (no modification) and 206 (partial) responses */
    ci_service_enable_204(srv_xdata);
    ci_service_enable_206(srv_xdata);

    /* ---------------------------------------------------------- */
    /* Step 1: Compile OTT regex pattern                          */
    /* ---------------------------------------------------------- */
    ret = regcomp(&ott_regex, "ott-[a-zA-Z0-9]{8}",
                  REG_EXTENDED | REG_ICASE);
    if (ret != 0) {
        char errbuf[256];
        regerror(ret, &ott_regex, errbuf, sizeof(errbuf));
        ci_debug_printf(1, "sentinel_resp: ERROR: "
            "Failed to compile OTT regex: %s\n", errbuf);
        return CI_ERROR;
    }
    ci_debug_printf(3, "sentinel_resp: OTT regex compiled\n");

    /* ---------------------------------------------------------- */
    /* Step 2: Load domain allowlist from environment             */
    /* ---------------------------------------------------------- */
    {
        const char *domains_env = getenv("POLIS_APPROVAL_DOMAINS");
        if (domains_env && strlen(domains_env) > 0) {
            char *domains_copy = strdup(domains_env);
            char *token = strtok(domains_copy, ",");
            while (token && allowed_domains_count < 32) {
                /* Trim whitespace */
                while (*token == ' ' || *token == '\t') token++;
                char *end = token + strlen(token) - 1;
                while (end > token &&
                       (*end == ' ' || *end == '\t' || *end == '\n'))
                    *end-- = '\0';

                if (strlen(token) > 0) {
                    allowed_domains[allowed_domains_count] = strdup(token);
                    ci_debug_printf(3, "sentinel_resp: "
                        "Loaded domain: %s\n",
                        allowed_domains[allowed_domains_count]);
                    allowed_domains_count++;
                }
                token = strtok(NULL, ",");
            }
            free(domains_copy);
            ci_debug_printf(2, "sentinel_resp: "
                "Loaded %d domain(s) from POLIS_APPROVAL_DOMAINS\n",
                allowed_domains_count);
        } else {
            /* Default: .api.telegram.org */
            allowed_domains[0] = strdup(".api.telegram.org");
            allowed_domains_count = 1;
            ci_debug_printf(3, "sentinel_resp: "
                "Using default domain: .api.telegram.org\n");
        }
    }

    /* ---------------------------------------------------------- */
    /* Step 3: Load clamd connection config from environment      */
    /* ---------------------------------------------------------- */
    {
        const char *host_env = getenv("POLIS_CLAMD_HOST");
        const char *port_env = getenv("POLIS_CLAMD_PORT");
        const char *socket_env = getenv("POLIS_CLAMD_SOCKET");

        if (socket_env && strlen(socket_env) > 0) {
            /* Explicit Unix socket path — use Unix socket mode */
            strncpy(clamd_socket_path, socket_env,
                    sizeof(clamd_socket_path) - 1);
            clamd_socket_path[sizeof(clamd_socket_path) - 1] = '\0';
            clamd_use_tcp = 0;
            ci_debug_printf(2, "sentinel_resp: clamd Unix socket: %s\n",
                clamd_socket_path);
        } else {
            /* Default: TCP connection to scanner:3310 */
            if (host_env && strlen(host_env) > 0) {
                strncpy(clamd_host, host_env,
                        sizeof(clamd_host) - 1);
                clamd_host[sizeof(clamd_host) - 1] = '\0';
            }
            if (port_env && strlen(port_env) > 0) {
                clamd_port = atoi(port_env);
                if (clamd_port <= 0 || clamd_port > 65535)
                    clamd_port = 3310;
            }
            clamd_use_tcp = 1;
            ci_debug_printf(2, "sentinel_resp: clamd TCP: %s:%d\n",
                clamd_host, clamd_port);
        }
    }

    /* ---------------------------------------------------------- */
    /* Step 4: Valkey lazy-init (MPMT fork-safe)                  */
    /* ---------------------------------------------------------- */
    /* Valkey connections are lazy-initialized on first use in child
     * processes. c-ICAP uses MPMT (pre-fork) model — connections
     * established here in the main process would be corrupted after
     * fork because OpenSSL/TLS state is not fork-safe.
     * ensure_valkey_connected() handles lazy init. */
    ci_debug_printf(2, "sentinel_resp: "
        "Valkey connection will be lazy-initialized\n");

    /* ---------------------------------------------------------- */
    /* Step 5: Initialize circuit breaker mutex                   */
    /* ---------------------------------------------------------- */
    pthread_mutex_init(&clamd_cb.mutex, NULL);
    ci_debug_printf(3, "sentinel_resp: Circuit breaker initialized\n");

    ci_debug_printf(2, "sentinel_resp: Service initialization complete\n");
    return CI_OK;
}

/**
 * sentinel_resp_close_service() - Clean up service resources
 *
 * Called once at c-ICAP shutdown. Performs:
 * 1. Free OTT regex
 * 2. Free domain allowlist
 * 3. Free Valkey connection
 * 4. Destroy mutexes
 */
static void sentinel_resp_close_service(void)
{
    int i;

    ci_debug_printf(2, "sentinel_resp: Closing service\n");

    /* ---------------------------------------------------------- */
    /* Step 1: Free OTT regex                                     */
    /* ---------------------------------------------------------- */
    regfree(&ott_regex);
    ci_debug_printf(3, "sentinel_resp: OTT regex freed\n");

    /* ---------------------------------------------------------- */
    /* Step 2: Free domain allowlist                              */
    /* ---------------------------------------------------------- */
    for (i = 0; i < allowed_domains_count; i++) {
        if (allowed_domains[i]) {
            free(allowed_domains[i]);
            allowed_domains[i] = NULL;
        }
    }
    allowed_domains_count = 0;
    ci_debug_printf(3, "sentinel_resp: Domain allowlist freed\n");

    /* ---------------------------------------------------------- */
    /* Step 3: Free Valkey connection                             */
    /* ---------------------------------------------------------- */
    pthread_mutex_lock(&valkey_mutex);
    if (valkey_ctx) {
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
        ci_debug_printf(3, "sentinel_resp: Valkey connection freed\n");
    }
    pthread_mutex_unlock(&valkey_mutex);

    /* ---------------------------------------------------------- */
    /* Step 4: Destroy mutexes                                    */
    /* ---------------------------------------------------------- */
    pthread_mutex_destroy(&valkey_mutex);
    pthread_mutex_destroy(&clamd_cb.mutex);
    ci_debug_printf(3, "sentinel_resp: Mutexes destroyed\n");

    ci_debug_printf(2, "sentinel_resp: Service closed\n");
}
/**
 * sentinel_resp_init_request_data() - Allocate per-request data
 *
 * Called for each RESPMOD request. Allocates and zero-initializes
 * the sentinel_resp_data_t structure.
 *
 * Returns: Pointer to allocated data, or NULL on failure
 */
static void *sentinel_resp_init_request_data(ci_request_t *req)
{
    sentinel_resp_data_t *data;

    /* Allocate per-request data structure */
    data = malloc(sizeof(sentinel_resp_data_t));
    if (!data) {
        ci_debug_printf(1, "sentinel_resp: ERROR: "
            "Failed to allocate request data\n");
        return NULL;
    }

    /* Zero-initialize all fields */
    memset(data, 0, sizeof(sentinel_resp_data_t));

    ci_debug_printf(5, "sentinel_resp: Request data initialized\n");
    return data;
}

/**
 * sentinel_resp_release_request_data() - Free per-request data
 *
 * Called when the request is complete. Frees all allocated resources:
 * - Body membuf
 * - Cached file
 * - Error page membuf
 * - Request data structure itself
 */
static void sentinel_resp_release_request_data(void *data)
{
    sentinel_resp_data_t *req_data = (sentinel_resp_data_t *)data;

    if (!req_data) {
        return;
    }

    /* ---------------------------------------------------------- */
    /* Free body membuf                                           */
    /* ---------------------------------------------------------- */
    if (req_data->body) {
        ci_membuf_free(req_data->body);
        req_data->body = NULL;
        ci_debug_printf(5, "sentinel_resp: Body membuf freed\n");
    }

    /* ---------------------------------------------------------- */
    /* Free cached file                                           */
    /* ---------------------------------------------------------- */
    if (req_data->cached) {
        ci_cached_file_destroy(req_data->cached);
        req_data->cached = NULL;
        ci_debug_printf(5, "sentinel_resp: Cached file freed\n");
    }

    /* ---------------------------------------------------------- */
    /* Free error page membuf                                     */
    /* ---------------------------------------------------------- */
    if (req_data->error_page) {
        ci_membuf_free(req_data->error_page);
        req_data->error_page = NULL;
        ci_debug_printf(5, "sentinel_resp: Error page freed\n");
    }

    /* ---------------------------------------------------------- */
    /* Free request data structure                                */
    /* ---------------------------------------------------------- */
    free(req_data);
    ci_debug_printf(5, "sentinel_resp: Request data released\n");
}

/* ========================================================================== */
/* Request Processing Callbacks                                               */
/* ========================================================================== */

/**
 * sentinel_resp_check_preview() - Extract headers and request full body
 *
 * Called after receiving response headers. Performs:
 * 1. Extract Host header from response headers
 * 2. Detect Content-Encoding: gzip flag
 * 3. Return CI_MOD_CONTINUE to receive full body
 *
 * Returns: CI_MOD_CONTINUE to receive full body
 */
static int sentinel_resp_check_preview(char *preview_data, int preview_data_len, ci_request_t *req)
{
    sentinel_resp_data_t *data;
    const char *header_val;
    ci_headers_list_t *resp_headers;

    /* ---------------------------------------------------------- */
    /* Get per-request data                                       */
    /* ---------------------------------------------------------- */
    data = ci_service_data(req);
    if (!data) {
        ci_debug_printf(1, "sentinel_resp: ERROR: "
            "No request data in check_preview\n");
        return CI_MOD_CONTINUE;
    }

    /* ---------------------------------------------------------- */
    /* Extract Host header from response headers                  */
    /* ---------------------------------------------------------- */
    resp_headers = ci_http_response_headers(req);
    if (resp_headers) {
        /* Try to get Host from response headers first */
        header_val = ci_headers_value(resp_headers, "Host");
        if (header_val) {
            strncpy(data->host, header_val, sizeof(data->host) - 1);
            data->host[sizeof(data->host) - 1] = '\0';
            ci_debug_printf(4, "sentinel_resp: Host from response: %s\n",
                data->host);
        }
    }

    /* If Host not in response headers, try request headers */
    if (data->host[0] == '\0') {
        ci_headers_list_t *req_headers = ci_http_request_headers(req);
        if (req_headers) {
            header_val = ci_headers_value(req_headers, "Host");
            if (header_val) {
                strncpy(data->host, header_val, sizeof(data->host) - 1);
                data->host[sizeof(data->host) - 1] = '\0';
                ci_debug_printf(4, "sentinel_resp: Host from request: %s\n",
                    data->host);
            }
        }
    }

    /* ---------------------------------------------------------- */
    /* Detect Content-Encoding: gzip flag                         */
    /* ---------------------------------------------------------- */
    if (resp_headers) {
        header_val = ci_headers_value(resp_headers, "Content-Encoding");
        if (header_val && strstr(header_val, "gzip")) {
            data->is_gzip = 1;
            ci_debug_printf(4, "sentinel_resp: "
                "Content-Encoding: gzip detected\n");
        }
    }

    /* ---------------------------------------------------------- */
    /* Return CI_MOD_CONTINUE to receive full body                */
    /* ---------------------------------------------------------- */
    ci_debug_printf(5, "sentinel_resp: check_preview complete, "
        "requesting full body\n");
    return CI_MOD_CONTINUE;
}

/* ========================================================================== */
/* Helper Functions - ClamAV INSTREAM Protocol                                */
/* ========================================================================== */

/**
 * clamd_scan_buffer() - Scan buffer via clamd INSTREAM protocol
 *
 * Connects to clamd via Unix domain socket and scans the provided buffer
 * using the INSTREAM protocol:
 * 1. Send "zINSTREAM\0" (10 bytes)
 * 2. Stream body as 4-byte big-endian length-prefixed 16KB chunks
 * 3. Send zero-length terminator (0x00000000)
 * 4. Read response line
 *
 * @param buf        Buffer to scan
 * @param len        Buffer length
 * @param result     Output buffer for clamd response
 * @param result_len Size of result buffer
 *
 * Returns:
 *   0  = clean (OK)
 *   1  = virus found (FOUND)
 *  -1  = error (connection failed, timeout, or protocol error)
 */
static int clamd_scan_buffer(const char *buf, size_t len,
                             char *result, size_t result_len)
{
    int fd = -1;
    struct timeval timeout;
    ssize_t n;
    size_t offset;
    int ret = -1;

    /* ---------------------------------------------------------- */
    /* Step 0: Check circuit breaker                              */
    /* ---------------------------------------------------------- */
    if (!clamd_cb_allow_request()) {
        ci_debug_printf(1, "sentinel_resp: clamd circuit breaker OPEN\n");
        return -1;  /* Caller returns 403 */
    }

    /* ---------------------------------------------------------- */
    /* Step 1: Connect to clamd (TCP or Unix socket)              */
    /* ---------------------------------------------------------- */
    if (clamd_use_tcp) {
        /* TCP connection to clamd */
        struct sockaddr_in tcp_addr;
        struct hostent *he;

        fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Failed to create TCP socket: %s\n", strerror(errno));
            clamd_cb_record_failure();
            return -1;
        }

        /* Set socket timeouts */
        timeout.tv_sec = CLAMD_TIMEOUT_SECS;
        timeout.tv_usec = 0;
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO,
                   &timeout, sizeof(timeout));
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO,
                   &timeout, sizeof(timeout));

        /* Resolve hostname */
        he = gethostbyname(clamd_host);
        if (!he) {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Failed to resolve clamd host '%s'\n", clamd_host);
            close(fd);
            clamd_cb_record_failure();
            return -1;
        }

        memset(&tcp_addr, 0, sizeof(tcp_addr));
        tcp_addr.sin_family = AF_INET;
        tcp_addr.sin_port = htons(clamd_port);
        memcpy(&tcp_addr.sin_addr, he->h_addr_list[0], he->h_length);

        if (connect(fd, (struct sockaddr *)&tcp_addr,
                    sizeof(tcp_addr)) < 0) {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Failed to connect to clamd at %s:%d: %s\n",
                clamd_host, clamd_port, strerror(errno));
            close(fd);
            clamd_cb_record_failure();
            return -1;
        }

        ci_debug_printf(4, "sentinel_resp: Connected to clamd at %s:%d\n",
            clamd_host, clamd_port);
    } else {
        /* Unix socket connection to clamd */
        struct sockaddr_un unix_addr;

        fd = socket(AF_UNIX, SOCK_STREAM, 0);
        if (fd < 0) {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Failed to create Unix socket: %s\n", strerror(errno));
            clamd_cb_record_failure();
            return -1;
        }

        /* Set socket timeouts */
        timeout.tv_sec = CLAMD_TIMEOUT_SECS;
        timeout.tv_usec = 0;
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO,
                   &timeout, sizeof(timeout));
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO,
                   &timeout, sizeof(timeout));

        memset(&unix_addr, 0, sizeof(unix_addr));
        unix_addr.sun_family = AF_UNIX;
        strncpy(unix_addr.sun_path, clamd_socket_path,
                sizeof(unix_addr.sun_path));
        unix_addr.sun_path[sizeof(unix_addr.sun_path) - 1] = '\0';

        if (connect(fd, (struct sockaddr *)&unix_addr,
                    sizeof(unix_addr)) < 0) {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Failed to connect to clamd at %s: %s\n",
                clamd_socket_path, strerror(errno));
            close(fd);
            clamd_cb_record_failure();
            return -1;
        }

        ci_debug_printf(4, "sentinel_resp: Connected to clamd at %s\n",
            clamd_socket_path);
    }

    /* ---------------------------------------------------------- */
    /* Step 2: Send "zINSTREAM\0" (10 bytes)                      */
    /* ---------------------------------------------------------- */
    n = write(fd, "zINSTREAM\0", 10);
    if (n != 10) {
        ci_debug_printf(1, "sentinel_resp: ERROR: "
            "Failed to send INSTREAM command: %s\n",
            n < 0 ? strerror(errno) : "short write");
        close(fd);
        clamd_cb_record_failure();
        return -1;
    }

    ci_debug_printf(5, "sentinel_resp: Sent zINSTREAM command\n");

    /* ---------------------------------------------------------- */
    /* Step 3: Stream body as 4-byte big-endian length-prefixed  */
    /*         16KB chunks                                        */
    /* ---------------------------------------------------------- */
    offset = 0;
    while (offset < len) {
        size_t chunk_size = len - offset;
        uint32_t chunk_size_be;
        unsigned char size_buf[4];

        /* Limit chunk to CLAMD_CHUNK_SIZE (16KB) */
        if (chunk_size > CLAMD_CHUNK_SIZE) {
            chunk_size = CLAMD_CHUNK_SIZE;
        }

        /* Convert chunk size to big-endian */
        chunk_size_be = htonl((uint32_t)chunk_size);
        memcpy(size_buf, &chunk_size_be, 4);

        /* Send 4-byte length prefix */
        n = write(fd, size_buf, 4);
        if (n != 4) {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Failed to send chunk size: %s\n",
                n < 0 ? strerror(errno) : "short write");
            close(fd);
            clamd_cb_record_failure();
            return -1;
        }

        /* Send chunk data */
        n = write(fd, buf + offset, chunk_size);
        if (n != (ssize_t)chunk_size) {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Failed to send chunk data: %s\n",
                n < 0 ? strerror(errno) : "short write");
            close(fd);
            clamd_cb_record_failure();
            return -1;
        }

        offset += chunk_size;
        ci_debug_printf(5, "sentinel_resp: Sent chunk: %zu bytes "
            "(total: %zu/%zu)\n", chunk_size, offset, len);
    }

    /* ---------------------------------------------------------- */
    /* Step 4: Send zero-length terminator (0x00000000)           */
    /* ---------------------------------------------------------- */
    {
        unsigned char zero[4] = {0, 0, 0, 0};
        n = write(fd, zero, 4);
        if (n != 4) {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Failed to send terminator: %s\n",
                n < 0 ? strerror(errno) : "short write");
            close(fd);
            clamd_cb_record_failure();
            return -1;
        }
    }

    ci_debug_printf(5, "sentinel_resp: Sent zero-length terminator\n");

    /* ---------------------------------------------------------- */
    /* Step 5: Read response line                                 */
    /* ---------------------------------------------------------- */
    {
        size_t result_offset = 0;
        char c;

        /* Read response line character by character until newline or null */
        while (result_offset < result_len - 1) {
            n = read(fd, &c, 1);
            if (n <= 0) {
                if (n < 0) {
                    ci_debug_printf(1, "sentinel_resp: ERROR: "
                        "Failed to read clamd response: %s\n",
                        strerror(errno));
                } else {
                    /* EOF — use whatever we have so far */
                    break;
                }
                close(fd);
                clamd_cb_record_failure();
                return -1;
            }

            if (c == '\n' || c == '\0') {
                break;  /* End of response line */
            }

            result[result_offset++] = c;
        }
        result[result_offset] = '\0';  /* Null-terminate */

        ci_debug_printf(4, "sentinel_resp: clamd response: %s\n", result);
    }

    /* ---------------------------------------------------------- */
    /* Step 6: Close socket                                       */
    /* ---------------------------------------------------------- */
    close(fd);
    ci_debug_printf(5, "sentinel_resp: Closed clamd socket\n");

    /* ---------------------------------------------------------- */
    /* Step 7: Parse response and record success/failure          */
    /* ---------------------------------------------------------- */
    if (strstr(result, "FOUND")) {
        /* Virus found */
        ci_debug_printf(3, "sentinel_resp: Virus detected: %s\n", result);
        clamd_cb_record_success();
        ret = 1;
    } else if (strstr(result, "OK")) {
        /* Clean */
        ci_debug_printf(4, "sentinel_resp: Scan clean\n");
        clamd_cb_record_success();
        ret = 0;
    } else {
        /* Unexpected response — treat as error */
        ci_debug_printf(1, "sentinel_resp: ERROR: "
            "Unexpected clamd response: %s\n", result);
        clamd_cb_record_failure();
        ret = -1;
    }

    return ret;
}

/* ========================================================================== */
/* Helper Functions - Valkey Connection Management                            */
/* ========================================================================== */

/**
 * valkey_init() - Initialize Valkey connection as governance-respmod
 *
 * Establishes TLS connection to Valkey and authenticates as governance-respmod.
 * Reads password from /run/secrets/valkey_respmod_password.
 *
 * This function handles its own mutex locking.
 *
 * Returns: 0 on success, -1 on failure
 */
static int valkey_init(void)
{
    const char *vk_host;
    int vk_port = 6379;
    const char *tls_cert;
    const char *tls_key;
    const char *tls_ca;
    redisSSLContext *ssl_ctx = NULL;
    redisSSLContextError ssl_err;
    redisReply *reply;
    FILE *fp;
    char password[256];
    size_t pass_len;

    /* Lock: all Valkey state modifications under mutex */
    pthread_mutex_lock(&valkey_mutex);

    /* Read Valkey host from environment (default: "state") */
    vk_host = getenv("VALKEY_HOST");
    if (!vk_host) vk_host = "state";

    /* Read Valkey port from environment */
    {
        const char *vk_port_str = getenv("VALKEY_PORT");
        if (vk_port_str) vk_port = atoi(vk_port_str);
    }

    /* Read TLS certificate paths from environment */
    tls_cert = getenv("VALKEY_TLS_CERT");
    tls_key  = getenv("VALKEY_TLS_KEY");
    tls_ca   = getenv("VALKEY_TLS_CA");

    /* Initialize OpenSSL for hiredis TLS */
    redisInitOpenSSL();

    /* Create TLS context with client certificates for mTLS */
    ssl_ctx = redisCreateSSLContext(
        tls_ca   ? tls_ca   : "/etc/valkey/tls/ca.crt",
        NULL,   /* capath — not used, single CA file */
        tls_cert ? tls_cert : "/etc/valkey/tls/client.crt",
        tls_key  ? tls_key  : "/etc/valkey/tls/client.key",
        NULL,   /* server_name — use default */
        &ssl_err);
    if (ssl_ctx == NULL) {
        ci_debug_printf(1, "sentinel_resp: WARNING: "
            "Failed to create TLS context for governance-respmod: %s — "
            "OTT approval unavailable\n",
            redisSSLContextGetError(ssl_err));
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }

    /* Establish TCP connection to Valkey */
    valkey_ctx = redisConnect(vk_host, vk_port);
    if (valkey_ctx == NULL || valkey_ctx->err) {
        ci_debug_printf(1, "sentinel_resp: WARNING: "
            "Cannot connect to Valkey at %s:%d for governance-respmod%s%s — "
            "OTT approval unavailable\n",
            vk_host, vk_port,
            valkey_ctx ? ": " : "",
            valkey_ctx ? valkey_ctx->errstr : "");
        if (valkey_ctx) {
            redisFree(valkey_ctx);
            valkey_ctx = NULL;
        }
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }

    /* Initiate TLS handshake on the connection */
    if (redisInitiateSSLWithContext(valkey_ctx, ssl_ctx) != REDIS_OK) {
        ci_debug_printf(1, "sentinel_resp: WARNING: "
            "TLS handshake failed with Valkey for governance-respmod: %s — "
            "OTT approval unavailable\n",
            valkey_ctx->errstr);
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }

    /* Read governance-respmod password from Docker secret file */
    fp = fopen("/run/secrets/valkey_respmod_password", "r");
    if (!fp) {
        ci_debug_printf(1, "sentinel_resp: WARNING: "
            "Cannot open /run/secrets/valkey_respmod_password — "
            "OTT approval unavailable\n");
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }

    memset(password, 0, sizeof(password));
    if (fgets(password, sizeof(password), fp) == NULL) {
        ci_debug_printf(1, "sentinel_resp: WARNING: "
            "Failed to read password from "
            "/run/secrets/valkey_respmod_password\n");
        fclose(fp);
        memset(password, 0, sizeof(password));
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
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

    /* Authenticate with ACL: AUTH governance-respmod <password> */
    reply = redisCommand(valkey_ctx,
        "AUTH governance-respmod %s", password);

    /* Scrub password from stack immediately after AUTH */
    memset(password, 0, sizeof(password));

    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "sentinel_resp: CRITICAL: "
            "Valkey ACL auth failed as governance-respmod%s%s — "
            "OTT approval unavailable\n",
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
        redisFreeSSLContext(ssl_ctx);
        pthread_mutex_unlock(&valkey_mutex);
        return -1;
    }
    freeReplyObject(reply);

    ci_debug_printf(3, "sentinel_resp: "
        "Authenticated as governance-respmod\n");

    ci_debug_printf(3, "sentinel_resp: "
        "Connected to Valkey at %s:%d as governance-respmod (TLS + ACL)\n",
        vk_host, vk_port);

    /* Note: ssl_ctx is owned by valkey_ctx after successful
     * redisInitiateSSLWithContext, so we don't free it here */

    pthread_mutex_unlock(&valkey_mutex);
    return 0;
}

/**
 * ensure_valkey_connected() - Lazy reconnect for governance-respmod
 *
 * Checks if valkey_ctx is connected. If not, attempts reconnection
 * via valkey_init(). Thread-safe with mutex.
 *
 * Returns: 1 if connected, 0 if unavailable
 */
static int ensure_valkey_connected(void)
{
    int connected;

    pthread_mutex_lock(&valkey_mutex);

    /* Check if context exists and is not in error state */
    if (valkey_ctx != NULL && valkey_ctx->err == 0) {
        /* Test connection with PING */
        redisReply *reply = redisCommand(valkey_ctx, "PING");
        if (reply != NULL && reply->type != REDIS_REPLY_ERROR) {
            freeReplyObject(reply);
            pthread_mutex_unlock(&valkey_mutex);
            return 1;  /* Connected */
        }
        if (reply) freeReplyObject(reply);

        /* PING failed — connection is stale, free it */
        ci_debug_printf(2, "sentinel_resp: "
            "governance-respmod connection stale, reconnecting\n");
        redisFree(valkey_ctx);
        valkey_ctx = NULL;
    }

    pthread_mutex_unlock(&valkey_mutex);

    /* Attempt reconnection (valkey_init handles its own locking) */
    connected = (valkey_init() == 0);

    return connected;
}

/* ========================================================================== */
/* Helper Functions - Circuit Breaker                                         */
/* ========================================================================== */

/**
 * clamd_cb_allow_request() - Check if circuit breaker allows request
 *
 * Circuit breaker states:
 * - CLOSED: Normal operation, all requests allowed
 * - OPEN: Too many failures, reject immediately
 * - HALF_OPEN: Recovery period, allow probe requests
 *
 * Returns: 1 if request allowed, 0 if rejected
 */
static int clamd_cb_allow_request(void)
{
    int allow = 0;

    pthread_mutex_lock(&clamd_cb.mutex);

    if (clamd_cb.state == CB_CLOSED) {
        /* Normal operation — allow all requests */
        allow = 1;
    } else if (clamd_cb.state == CB_OPEN) {
        /* Circuit open — check if recovery period has elapsed */
        if (time(NULL) - clamd_cb.last_failure >= CB_RECOVERY_SECS) {
            /* Transition to half-open — allow probe request */
            clamd_cb.state = CB_HALF_OPEN;
            ci_debug_printf(3, "sentinel_resp: Circuit breaker "
                "transitioning to HALF_OPEN\n");
            allow = 1;
        } else {
            /* Still in open state — reject immediately */
            ci_debug_printf(4, "sentinel_resp: Circuit breaker OPEN, "
                "rejecting request\n");
            allow = 0;
        }
    } else {
        /* CB_HALF_OPEN — allow probe request */
        ci_debug_printf(4, "sentinel_resp: Circuit breaker HALF_OPEN, "
            "allowing probe request\n");
        allow = 1;
    }

    pthread_mutex_unlock(&clamd_cb.mutex);
    return allow;
}

/**
 * clamd_cb_record_success() - Record successful clamd connection
 *
 * Resets failure count and transitions circuit breaker to CLOSED state.
 */
static void clamd_cb_record_success(void)
{
    pthread_mutex_lock(&clamd_cb.mutex);

    if (clamd_cb.state != CB_CLOSED) {
        ci_debug_printf(3, "sentinel_resp: Circuit breaker "
            "transitioning to CLOSED (success)\n");
    }

    clamd_cb.failure_count = 0;
    clamd_cb.state = CB_CLOSED;

    pthread_mutex_unlock(&clamd_cb.mutex);
}

/**
 * clamd_cb_record_failure() - Record failed clamd connection
 *
 * Increments failure count and opens circuit breaker if threshold exceeded.
 */
static void clamd_cb_record_failure(void)
{
    pthread_mutex_lock(&clamd_cb.mutex);

    clamd_cb.failure_count++;
    clamd_cb.last_failure = time(NULL);

    ci_debug_printf(3, "sentinel_resp: Circuit breaker failure count: %d\n",
        clamd_cb.failure_count);

    if (clamd_cb.failure_count >= CB_FAILURE_THRESHOLD) {
        if (clamd_cb.state != CB_OPEN) {
            ci_debug_printf(2, "sentinel_resp: Circuit breaker "
                "transitioning to OPEN (threshold exceeded)\n");
        }
        clamd_cb.state = CB_OPEN;
    }

    pthread_mutex_unlock(&clamd_cb.mutex);
}

/**
 * is_allowed_domain() — Dot-boundary domain matching (CWE-346)
 *
 * Checks whether the given host matches any entry in the domain allowlist.
 * Implements two matching modes for dot-prefixed entries:
 *
 * 1. Exact match against the bare domain (without leading dot)
 *    Example: host "slack.com" matches entry ".slack.com"
 *
 * 2. Suffix match with dot-boundary enforcement
 *    Example: host "api.slack.com" matches entry ".slack.com"
 *    Counter-example: host "evil-slack.com" does NOT match ".slack.com"
 *
 * Non-dot-prefixed entries require exact match only.
 *
 * All comparisons are case-insensitive per DNS conventions.
 *
 * Returns: 1 if host matches allowlist, 0 otherwise
 *
 * Validates: Requirements 2.6, 2.7, 2.15
 */

/*
 * is_known_package_registry() — Check if host is a known package registry.
 *
 * Used to decide fail-open vs fail-closed when ClamAV times out.
 * Known package registries are trusted sources where a ClamAV timeout
 * should not block the download (fail-open), while unknown domains
 * remain fail-closed for security.
 */
static int is_known_package_registry(const char *host)
{
    static const char *registries[] = {
        ".registry.npmjs.org",
        ".deb.nodesource.com",
        ".deb.debian.org",
        ".bun.sh",
        ".github.com",
        ".githubusercontent.com",
        ".pypi.org",
        ".files.pythonhosted.org",
        ".crates.io",
        ".static.crates.io",
        ".rubygems.org",
        NULL
    };
    size_t hlen;
    int i;

    if (host == NULL || host[0] == '\0')
        return 0;

    hlen = strlen(host);

    for (i = 0; registries[i] != NULL; i++) {
        size_t dlen = strlen(registries[i]);

        /* Suffix match */
        if (hlen >= dlen &&
            strcasecmp(host + (hlen - dlen), registries[i]) == 0)
            return 1;

        /* Exact match without leading dot */
        if (strcasecmp(host, registries[i] + 1) == 0)
            return 1;
    }

    return 0;
}

static int is_allowed_domain(const char *host)
{
    int i;
    size_t host_len;
    size_t entry_len;

    if (host == NULL || host[0] == '\0')
        return 0;

    host_len = strlen(host);

    for (i = 0; i < allowed_domains_count; i++) {
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

/* ========================================================================== */
/* OTT Approval Flow                                                          */
/* ========================================================================== */

/**
 * process_ott_approval() - Execute the 8-step OTT approval flow
 *
 * Ported from srv_polis_approval.c line 537. Implements atomic approval
 * processing with the following steps:
 *   1. GET OTT mapping from Valkey
 *   2. Check time-gate (armed_after)
 *   3. Check context binding (origin_host matches resp_host)
 *   4. Check blocked key exists
 *   5. Preserve audit data (GET blocked request)
 *   6. ZADD audit log entry
 *   7. DEL blocked key + SETEX approved key
 *   8. DEL OTT key
 *
 * @param ott_code: The OTT code found in the response body
 * @param resp_host: The Host header from the response
 *
 * Returns: 1 on successful approval, 0 if OTT invalid/expired, -1 on error
 *
 * Validates: Requirements 2.8, 2.9, 2.10
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

    /* Destination host extracted from blocked request */
    char blocked_dest_host[256];

    /* Parsed fields from OTT mapping JSON */
    char parsed_request_id[32];
    char parsed_origin_host[256];
    long parsed_armed_after = 0;

    time_t now;

    if (ott_code == NULL || resp_host == NULL) {
        ci_debug_printf(1, "sentinel_resp: "
            "process_ott_approval: NULL parameter\n");
        return -1;
    }

    blocked_dest_host[0] = '\0';

    /* Lazy reconnect if connection was lost or not yet established */
    if (!ensure_valkey_connected()) {
        ci_debug_printf(1, "sentinel_resp: "
            "process_ott_approval: Valkey unavailable — "
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
        ci_debug_printf(1, "sentinel_resp: "
            "Valkey GET failed for OTT '%s'%s%s\n",
            ott_code,
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        return -1;
    }

    if (reply->type == REDIS_REPLY_NIL || reply->str == NULL) {
        ci_debug_printf(3, "sentinel_resp: "
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
        ci_debug_printf(0, "sentinel_resp: CRITICAL: "
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
            ci_debug_printf(1, "sentinel_resp: "
                "Malformed OTT JSON — missing request_id "
                "for OTT '%s'\n", ott_code);
            free(ott_json);
            return -1;
        }
        p += strlen("\"request_id\":\"");
        end = strchr(p, '"');
        if (end == NULL || (size_t)(end - p) >=
                sizeof(parsed_request_id)) {
            ci_debug_printf(1, "sentinel_resp: "
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
            ci_debug_printf(1, "sentinel_resp: "
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
            ci_debug_printf(1, "sentinel_resp: "
                "Malformed OTT JSON — missing origin_host "
                "for OTT '%s'\n", ott_code);
            free(ott_json);
            return -1;
        }
        p += strlen("\"origin_host\":\"");
        end = strchr(p, '"');
        if (end == NULL || (size_t)(end - p) >=
                sizeof(parsed_origin_host)) {
            ci_debug_printf(1, "sentinel_resp: "
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

    ci_debug_printf(3, "sentinel_resp: "
        "OTT '%s' → request_id='%s', origin_host='%s', "
        "armed_after=%ld\n",
        ott_code, parsed_request_id,
        parsed_origin_host, parsed_armed_after);


    /* ---------------------------------------------------------- */
    /* Step 2: Check time-gate — now >= armed_after (Req 2.9)     */
    /* If time-gate has NOT elapsed, ignore the OTT.              */
    /* This prevents self-approval via sendMessage echo.          */
    /* ---------------------------------------------------------- */
    now = time(NULL);
    if ((long)now < parsed_armed_after) {
        ci_debug_printf(3, "sentinel_resp: "
            "OTT '%s' time-gate not elapsed — "
            "now=%ld < armed_after=%ld — "
            "ignoring (echo protection)\n",
            ott_code, (long)now, parsed_armed_after);
        return 0;
    }

    /* ---------------------------------------------------------- */
    /* Step 3: Check context binding (Req 2.10)                   */
    /* resp_host must match origin_host from OTT mapping.         */
    /* Prevents cross-channel OTT replay attacks.                 */
    /* ---------------------------------------------------------- */
    if (strcasecmp(resp_host, parsed_origin_host) != 0) {
        ci_debug_printf(1, "sentinel_resp: "
            "OTT '%s' context binding FAILED — "
            "resp_host='%s' != origin_host='%s' — "
            "rejecting (cross-channel replay prevention)\n",
            ott_code, resp_host, parsed_origin_host);
        return 0;
    }

    ci_debug_printf(3, "sentinel_resp: "
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
        ci_debug_printf(1, "sentinel_resp: "
            "Valkey EXISTS failed for '%s'%s%s\n",
            blocked_key,
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        return -1;
    }

    if (reply->integer == 0) {
        ci_debug_printf(3, "sentinel_resp: "
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
        ci_debug_printf(1, "sentinel_resp: "
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
        ci_debug_printf(1, "sentinel_resp: "
            "Blocked data for '%s' is empty or "
            "strdup failed — proceeding without "
            "audit data\n", parsed_request_id);
        /* Non-fatal: proceed with approval but log warning */
        blocked_data = strdup("{}");
        if (blocked_data == NULL) {
            ci_debug_printf(0, "sentinel_resp: CRITICAL: "
                "strdup failed for fallback blocked_data\n");
            return -1;
        }
    }

    ci_debug_printf(3, "sentinel_resp: "
        "Preserved blocked data for '%s' "
        "(audit trail)\n", parsed_request_id);

    /* ---------------------------------------------------------- */
    /* Step 5b: Extract destination host from blocked request      */
    /* The blocked request JSON contains a "destination" field     */
    /* with the URL that was blocked (e.g. https://httpbin.org/x). */
    /* We parse the host from it for the host-based approval key.  */
    /* ---------------------------------------------------------- */
    {
        const char *dp = strstr(blocked_data,
                                "\"destination\":\"");
        if (dp) {
            const char *host_start;
            const char *host_end;
            size_t hlen;

            dp += strlen("\"destination\":\"");

            /* Skip scheme (https:// or http://) */
            host_start = strstr(dp, "://");
            if (host_start)
                host_start += 3;
            else
                host_start = dp;

            /* Find end of host: slash, colon, quote */
            host_end = host_start;
            while (*host_end && *host_end != '/' &&
                   *host_end != ':' && *host_end != '"')
                host_end++;

            hlen = (size_t)(host_end - host_start);
            if (hlen > 0 &&
                hlen < sizeof(blocked_dest_host)) {
                memcpy(blocked_dest_host,
                       host_start, hlen);
                blocked_dest_host[hlen] = '\0';
                ci_debug_printf(3, "sentinel_resp: "
                    "Blocked destination host: '%s'\n",
                    blocked_dest_host);
            }
        }
    }

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
            ci_debug_printf(1, "sentinel_resp: WARNING: "
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
            ci_debug_printf(0, "sentinel_resp: CRITICAL: "
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
            ci_debug_printf(1, "sentinel_resp: WARNING: "
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
        ci_debug_printf(3, "sentinel_resp: "
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
    /* Step 7: DEL blocked key, SETEX approved key (Req 2.8)      */
    /* Now safe to destroy source data — audit is persisted.      */
    /* Approval key has 5-minute TTL (APPROVAL_TTL_SECS = 300)    */
    /* ---------------------------------------------------------- */

    /* DEL the blocked key */
    reply = redisCommand(valkey_ctx,
                         "DEL %s", blocked_key);
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "sentinel_resp: "
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
        ci_debug_printf(1, "sentinel_resp: "
            "Valkey SETEX failed for '%s'%s%s\n",
            approved_key,
            reply ? ": " : "",
            reply ? reply->str : "");
        if (reply) freeReplyObject(reply);
        return -1;
    }
    freeReplyObject(reply);
    reply = NULL;

    ci_debug_printf(3, "sentinel_resp: "
        "Approved '%s' — SETEX with %ds TTL\n",
        parsed_request_id, APPROVAL_TTL_SECS);

    /* ---------------------------------------------------------- */
    /* Step 7b: SETEX host-based approval key (Req 2.8)           */
    /* Uses the blocked request's destination host so the DLP      */
    /* REQMOD module can allow retries to the same host.           */
    /* Falls back to origin_host if destination not available.     */
    /* ---------------------------------------------------------- */
    {
        char host_key[320];
        const char *approval_host;

        approval_host = (blocked_dest_host[0] != '\0')
                        ? blocked_dest_host
                        : parsed_origin_host;

        snprintf(host_key, sizeof(host_key),
                 "polis:approved:host:%s", approval_host);

        reply = redisCommand(valkey_ctx,
            "SETEX %s %d approved",
            host_key, APPROVAL_TTL_SECS);
        if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
            ci_debug_printf(1, "sentinel_resp: WARNING: "
                "Failed to SETEX host approval key '%s'%s%s\n",
                host_key,
                reply ? ": " : "",
                reply ? reply->str : "");
            /* Non-fatal: per-request approval still works */
        } else {
            ci_debug_printf(3, "sentinel_resp: "
                "Host approval key '%s' set with %ds TTL\n",
                host_key, APPROVAL_TTL_SECS);
        }
        if (reply) freeReplyObject(reply);
        reply = NULL;
    }


    /* ---------------------------------------------------------- */
    /* Step 8: DEL OTT key — consume the one-time token           */
    /* Done last so that if earlier steps fail, the OTT remains   */
    /* available for retry.                                       */
    /* ---------------------------------------------------------- */
    reply = redisCommand(valkey_ctx, "DEL %s", ott_key);
    if (reply == NULL || reply->type == REDIS_REPLY_ERROR) {
        ci_debug_printf(1, "sentinel_resp: WARNING: "
            "Failed to DEL OTT key '%s'%s%s — "
            "approval still valid, OTT will expire\n",
            ott_key,
            reply ? ": " : "",
            reply ? reply->str : "");
    } else {
        ci_debug_printf(3, "sentinel_resp: "
            "Deleted OTT key '%s'\n", ott_key);
    }
    if (reply) freeReplyObject(reply);

    ci_debug_printf(3, "sentinel_resp: "
        "OTT '%s' → request_id '%s' approved "
        "via proxy (origin: %s)\n",
        ott_code, parsed_request_id,
        parsed_origin_host);

    return 1;
}

/* ========================================================================== */
/* Helper Functions - Gzip Decompression/Compression                          */
/* ========================================================================== */

/* Decompression bomb defense limits */
#define MAX_DECOMPRESS_SIZE   (10 * 1024 * 1024)  /* 10MB absolute cap */
#define MAX_DECOMPRESS_RATIO  100                   /* 100:1 max ratio */

/**
 * decompress_gzip() - Inflate gzip body into plain text buffer
 *
 * Implements three-layer decompression bomb defense (CWE-409):
 * 1. Absolute size cap: 10MB maximum decompressed size
 * 2. Ratio check: 100:1 maximum compression ratio
 * 3. Incremental validation: checks limits during decompression
 *
 * @param in:      Input gzip-compressed buffer
 * @param in_len:  Input buffer length
 * @param out:     Output pointer (allocated by this function)
 * @param out_len: Output length (set by this function)
 *
 * Returns:
 *   0  = success (out and out_len are set)
 *  -1  = decompression error (zlib failure)
 *  -2  = decompression bomb detected (size or ratio exceeded)
 *
 * Caller must free(*out) on success.
 *
 * Validates: Requirements 2.12, H1 (Security Hardening)
 */
static int decompress_gzip(const char *in, size_t in_len,
                          char **out, size_t *out_len)
{
    z_stream strm;
    int ret;
    size_t alloc;
    char *buf;
    size_t total;

    if (in == NULL || out == NULL || out_len == NULL) {
        ci_debug_printf(1, "sentinel_resp: "
            "decompress_gzip: NULL parameter\n");
        return -1;
    }

    /* ---------------------------------------------------------- */
    /* Initialize zlib inflate stream with gzip window bits       */
    /* Window bits = 16 + MAX_WBITS enables gzip header parsing   */
    /* ---------------------------------------------------------- */
    memset(&strm, 0, sizeof(strm));
    ret = inflateInit2(&strm, 16 + MAX_WBITS);
    if (ret != Z_OK) {
        ci_debug_printf(1, "sentinel_resp: "
            "inflateInit2 failed: %d\n", ret);
        return -1;
    }

    /* ---------------------------------------------------------- */
    /* Allocate initial output buffer (4x input size estimate)    */
    /* Cap at MAX_DECOMPRESS_SIZE to prevent huge allocations     */
    /* ---------------------------------------------------------- */
    alloc = in_len * 4;
    if (alloc > MAX_DECOMPRESS_SIZE) {
        alloc = MAX_DECOMPRESS_SIZE;
    }
    if (alloc < 4096) {
        alloc = 4096;  /* Minimum buffer size */
    }

    buf = malloc(alloc);
    if (buf == NULL) {
        ci_debug_printf(1, "sentinel_resp: "
            "malloc failed for decompression buffer\n");
        inflateEnd(&strm);
        return -1;
    }

    /* ---------------------------------------------------------- */
    /* Decompress in a loop, growing buffer as needed              */
    /* ---------------------------------------------------------- */
    strm.next_in = (Bytef *)in;
    strm.avail_in = in_len;
    total = 0;

    while (1) {
        /* Set output pointer to current position in buffer */
        strm.next_out = (Bytef *)(buf + total);
        strm.avail_out = alloc - total;

        /* Inflate next chunk */
        ret = inflate(&strm, Z_NO_FLUSH);

        /* Update total bytes decompressed */
        total = alloc - strm.avail_out;

        /* ---------------------------------------------------------- */
        /* Layer 1: Absolute size cap (10MB)                          */
        /* ---------------------------------------------------------- */
        if (total > MAX_DECOMPRESS_SIZE) {
            ci_debug_printf(1, "sentinel_resp: DECOMP_BOMB "
                "size=%zu > %d, aborting\n",
                total, MAX_DECOMPRESS_SIZE);
            inflateEnd(&strm);
            free(buf);
            return -2;  /* Bomb detected */
        }

        /* ---------------------------------------------------------- */
        /* Layer 2: Ratio check (100:1)                               */
        /* ---------------------------------------------------------- */
        if (in_len > 0 && total / in_len > MAX_DECOMPRESS_RATIO) {
            ci_debug_printf(1, "sentinel_resp: DECOMP_BOMB "
                "ratio=%zu:1 > %d:1, aborting\n",
                total / in_len, MAX_DECOMPRESS_RATIO);
            inflateEnd(&strm);
            free(buf);
            return -2;  /* Bomb detected */
        }

        /* Check inflate return code */
        if (ret == Z_STREAM_END) {
            /* Decompression complete */
            break;
        }

        if (ret != Z_OK) {
            /* Decompression error */
            ci_debug_printf(1, "sentinel_resp: "
                "inflate failed: %d (%s)\n",
                ret, strm.msg ? strm.msg : "unknown");
            inflateEnd(&strm);
            free(buf);
            return -1;
        }

        /* ---------------------------------------------------------- */
        /* Grow buffer if needed (double size, capped at limit)       */
        /* ---------------------------------------------------------- */
        if (strm.avail_out == 0) {
            size_t new_alloc = alloc * 2;
            char *new_buf;

            /* Cap growth at MAX_DECOMPRESS_SIZE + 1 to trigger
             * size check on next iteration */
            if (new_alloc > MAX_DECOMPRESS_SIZE) {
                new_alloc = MAX_DECOMPRESS_SIZE + 1;
            }

            new_buf = realloc(buf, new_alloc);
            if (new_buf == NULL) {
                ci_debug_printf(1, "sentinel_resp: "
                    "realloc failed for decompression buffer\n");
                inflateEnd(&strm);
                free(buf);
                return -1;
            }

            buf = new_buf;
            alloc = new_alloc;
        }
    }

    /* ---------------------------------------------------------- */
    /* Clean up zlib stream                                        */
    /* ---------------------------------------------------------- */
    inflateEnd(&strm);

    /* ---------------------------------------------------------- */
    /* Return decompressed data                                    */
    /* ---------------------------------------------------------- */
    *out = buf;
    *out_len = total;

    ci_debug_printf(4, "sentinel_resp: "
        "Decompressed %zu → %zu bytes (ratio %.1f:1)\n",
        in_len, total,
        in_len > 0 ? (double)total / in_len : 0.0);

    return 0;
}

/**
 * compress_gzip() - Deflate plain text back to gzip
 *
 * Compresses plain text buffer into gzip format using zlib.
 * Uses default compression level (Z_DEFAULT_COMPRESSION = 6).
 *
 * @param in:      Input plain text buffer
 * @param in_len:  Input buffer length
 * @param out:     Output pointer (allocated by this function)
 * @param out_len: Output length (set by this function)
 *
 * Returns:
 *   0  = success (out and out_len are set)
 *  -1  = compression error (zlib failure)
 *
 * Caller must free(*out) on success.
 *
 * Validates: Requirements 2.12
 */
static int compress_gzip(const char *in, size_t in_len,
                        char **out, size_t *out_len)
{
    z_stream strm;
    int ret;
    size_t alloc;
    char *buf;
    size_t total;

    if (in == NULL || out == NULL || out_len == NULL) {
        ci_debug_printf(1, "sentinel_resp: "
            "compress_gzip: NULL parameter\n");
        return -1;
    }

    /* ---------------------------------------------------------- */
    /* Initialize zlib deflate stream with gzip window bits       */
    /* Window bits = 16 + MAX_WBITS enables gzip header/trailer   */
    /* ---------------------------------------------------------- */
    memset(&strm, 0, sizeof(strm));
    ret = deflateInit2(&strm,
                      Z_DEFAULT_COMPRESSION,  /* compression level */
                      Z_DEFLATED,             /* method */
                      16 + MAX_WBITS,         /* gzip format */
                      8,                      /* mem level */
                      Z_DEFAULT_STRATEGY);    /* strategy */
    if (ret != Z_OK) {
        ci_debug_printf(1, "sentinel_resp: "
            "deflateInit2 failed: %d\n", ret);
        return -1;
    }

    /* ---------------------------------------------------------- */
    /* Allocate output buffer (estimate: input size + 1KB header) */
    /* Compressed data is usually smaller, but we allocate        */
    /* input size + overhead to handle worst-case expansion       */
    /* ---------------------------------------------------------- */
    alloc = in_len + 1024;
    buf = malloc(alloc);
    if (buf == NULL) {
        ci_debug_printf(1, "sentinel_resp: "
            "malloc failed for compression buffer\n");
        deflateEnd(&strm);
        return -1;
    }

    /* ---------------------------------------------------------- */
    /* Compress in a single pass                                   */
    /* ---------------------------------------------------------- */
    strm.next_in = (Bytef *)in;
    strm.avail_in = in_len;
    strm.next_out = (Bytef *)buf;
    strm.avail_out = alloc;

    ret = deflate(&strm, Z_FINISH);

    if (ret == Z_STREAM_END) {
        /* Compression complete */
        total = alloc - strm.avail_out;
    } else if (ret == Z_OK) {
        /* Buffer too small — grow and retry */
        size_t new_alloc = alloc * 2;
        char *new_buf = realloc(buf, new_alloc);

        if (new_buf == NULL) {
            ci_debug_printf(1, "sentinel_resp: "
                "realloc failed for compression buffer\n");
            deflateEnd(&strm);
            free(buf);
            return -1;
        }

        buf = new_buf;
        alloc = new_alloc;

        /* Continue compression */
        strm.next_out = (Bytef *)(buf + (alloc / 2));
        strm.avail_out = alloc / 2;

        ret = deflate(&strm, Z_FINISH);
        if (ret != Z_STREAM_END) {
            ci_debug_printf(1, "sentinel_resp: "
                "deflate failed after realloc: %d (%s)\n",
                ret, strm.msg ? strm.msg : "unknown");
            deflateEnd(&strm);
            free(buf);
            return -1;
        }

        total = alloc - strm.avail_out;
    } else {
        /* Compression error */
        ci_debug_printf(1, "sentinel_resp: "
            "deflate failed: %d (%s)\n",
            ret, strm.msg ? strm.msg : "unknown");
        deflateEnd(&strm);
        free(buf);
        return -1;
    }

    /* ---------------------------------------------------------- */
    /* Clean up zlib stream                                        */
    /* ---------------------------------------------------------- */
    deflateEnd(&strm);

    /* ---------------------------------------------------------- */
    /* Return compressed data                                      */
    /* ---------------------------------------------------------- */
    *out = buf;
    *out_len = total;

    ci_debug_printf(4, "sentinel_resp: "
        "Compressed %zu → %zu bytes (ratio %.1f:1)\n",
        in_len, total,
        total > 0 ? (double)in_len / total : 0.0);

    return 0;
}

/* ========================================================================== */
/* Request Processing - Main Pipeline                                         */
/* ========================================================================== */

/**
 * sentinel_resp_process() - Main processing pipeline
 *
 * Called after all body data has been received (eof set).
 * Implements the complete RESPMOD processing pipeline:
 *
 * 1. ClamAV scan (all responses, regardless of domain):
 *    - Check circuit breaker state
 *    - Call clamd_scan_buffer() via Unix socket
 *    - If FOUND → return 403 (virus blocked)
 *    - If connection fails → return 403 (fail-closed)
 *    - If OK → proceed to OTT scan
 *
 * 2. OTT scan (only if ClamAV passed AND host in allowlist):
 *    - If gzip → decompress body (bomb defense: 10MB cap, 100:1 ratio)
 *    - If decompression bomb → skip OTT scan, pass original compressed body
 *    - Scan for ott-[a-zA-Z0-9]{8} regex
 *    - For each match → process_ott_approval() (MULTI/EXEC atomic)
 *    - Strip OTT codes from body (replace with asterisks)
 *    - If was gzip → recompress
 *
 * 3. Pass through (if ClamAV passed AND host NOT in allowlist):
 *    - No OTT processing, body passes through unmodified
 *
 * @param req: ICAP request context
 *
 * Returns: CI_MOD_DONE on success, CI_ERROR on failure
 *
 * Validates: Requirements 2.5, 2.6, 2.7, 2.8, 2.11, 2.14, 2.15
 */
static int sentinel_resp_process(ci_request_t *req)
{
    sentinel_resp_data_t *data;
    char clamd_result[CLAMD_MAX_RESPONSE];
    int scan_ret;
    const char *body_raw;
    size_t body_len;
    char *decompressed = NULL;
    size_t decompressed_len = 0;
    int was_decompressed = 0;
    regmatch_t match;
    const char *scan_buf;
    size_t scan_len;
    int ott_count = 0;

    /* ---------------------------------------------------------- */
    /* Get per-request data                                       */
    /* ---------------------------------------------------------- */
    data = ci_service_data(req);
    if (!data) {
        ci_debug_printf(1, "sentinel_resp: ERROR: "
            "No request data in sentinel_resp_process\n");
        return CI_ERROR;
    }

    /* ---------------------------------------------------------- */
    /* Ensure we have body data                                    */
    /* ---------------------------------------------------------- */
    if (!data->body || data->total_body_len == 0) {
        ci_debug_printf(4, "sentinel_resp: "
            "No body data — passing through\n");
        return ci_req_allow204(req) ? CI_MOD_ALLOW204 : CI_MOD_DONE;
    }

    /* ---------------------------------------------------------- */
    /* Fallback host extraction (no_preview mode)                  */
    /* When g3proxy sends no_preview:true, check_preview may not   */
    /* be called, leaving data->host empty. Extract it here.       */
    /* ---------------------------------------------------------- */
    if (data->host[0] == '\0') {
        const char *hdr_val;
        ci_headers_list_t *rh;

        rh = ci_http_response_headers(req);
        if (rh) {
            hdr_val = ci_headers_value(rh, "Host");
            if (hdr_val) {
                strncpy(data->host, hdr_val,
                        sizeof(data->host) - 1);
                data->host[sizeof(data->host) - 1] = '\0';
            }
        }
        if (data->host[0] == '\0') {
            rh = ci_http_request_headers(req);
            if (rh) {
                hdr_val = ci_headers_value(rh, "Host");
                if (hdr_val) {
                    strncpy(data->host, hdr_val,
                            sizeof(data->host) - 1);
                    data->host[sizeof(data->host) - 1] = '\0';
                }
            }
        }
        if (data->host[0] == '\0') {
            /* Last resort: use ci_http_request_get_header */
            hdr_val = ci_http_request_get_header(req,
                                                 "Host");
            if (hdr_val) {
                strncpy(data->host, hdr_val,
                        sizeof(data->host) - 1);
                data->host[sizeof(data->host) - 1] = '\0';
            }
        }
        ci_debug_printf(3, "sentinel_resp: "
            "Fallback host extraction: '%s'\n",
            data->host);
    }

    /* ---------------------------------------------------------- */
    /* Fallback gzip detection                                     */
    /* ---------------------------------------------------------- */
    if (!data->is_gzip) {
        ci_headers_list_t *rh = ci_http_response_headers(req);
        if (rh) {
            const char *ce = ci_headers_value(rh,
                                 "Content-Encoding");
            if (ce && strstr(ce, "gzip")) {
                data->is_gzip = 1;
                ci_debug_printf(3, "sentinel_resp: "
                    "Fallback gzip detection: yes\n");
            }
        }
    }

    body_raw = (const char *)ci_membuf_raw(data->body);
    body_len = ci_membuf_size(data->body);

    ci_debug_printf(3, "sentinel_resp: "
        "Processing response: host=%s, size=%zu, gzip=%d\n",
        data->host, body_len, data->is_gzip);

    /* ========================================================================== */
    /* STEP 1: ClamAV Scan (ALL responses, regardless of domain)                 */
    /* ========================================================================== */

    ci_debug_printf(4, "sentinel_resp: "
        "Starting ClamAV scan (%zu bytes)\n", body_len);

    scan_ret = clamd_scan_buffer(body_raw, body_len,
                                 clamd_result, sizeof(clamd_result));

    if (scan_ret == 1) {
        /* ---------------------------------------------------------- */
        /* Virus found — return 403 with error page                   */
        /* ---------------------------------------------------------- */
        ci_debug_printf(2, "sentinel_resp: "
            "Virus detected: %s — blocking response\n",
            clamd_result);

        data->virus_found = 1;
        strncpy(data->virus_name, clamd_result,
                sizeof(data->virus_name) - 1);
        data->virus_name[sizeof(data->virus_name) - 1] = '\0';

        /* Create error page */
        data->error_page = ci_membuf_new_sized(4096);
        if (data->error_page) {
            const char *error_html =
                "HTTP/1.1 403 Forbidden\r\n"
                "Content-Type: text/html\r\n"
                "Connection: close\r\n"
                "\r\n"
                "<!DOCTYPE html>\n"
                "<html><head><title>Virus Detected</title></head>\n"
                "<body>\n"
                "<h1>403 Forbidden - Virus Detected</h1>\n"
                "<p>The requested content was blocked by antivirus scanning.</p>\n"
                "<p>Threat: %s</p>\n"
                "</body></html>\n";

            char error_buf[4096];
            snprintf(error_buf, sizeof(error_buf), error_html,
                    data->virus_name);
            ci_membuf_write(data->error_page, error_buf,
                           strlen(error_buf), 0);
        }

        /* Modify response to 403 */
        ci_http_response_reset_headers(req);
        ci_http_response_create(req, 1, 1);
        ci_http_response_add_header(req, "HTTP/1.1 403 Forbidden");
        ci_http_response_add_header(req, "Content-Type: text/html");
        ci_http_response_add_header(req, "Connection: close");

        return CI_MOD_DONE;

    } else if (scan_ret == -1) {
        /* ---------------------------------------------------------- */
        /* ClamAV scan failed — fail-open for known package registries */
        /* (timeout on large tarballs), fail-closed for everything else */
        /* ---------------------------------------------------------- */
        if (is_known_package_registry(data->host)) {
            ci_debug_printf(1, "sentinel_resp: WARNING: "
                "ClamAV scan failed for known registry '%s' — "
                "failing open (package download)\n", data->host);
            /* Fall through to OTT scan / pass-through */
        } else {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "ClamAV scan failed — failing closed\n");

            /* Create error page */
            data->error_page = ci_membuf_new_sized(4096);
            if (data->error_page) {
                const char *error_html =
                    "HTTP/1.1 403 Forbidden\r\n"
                    "Content-Type: text/html\r\n"
                    "Connection: close\r\n"
                    "\r\n"
                    "<!DOCTYPE html>\n"
                    "<html><head><title>Scanner Unavailable</title></head>\n"
                    "<body>\n"
                    "<h1>403 Forbidden - Scanner Unavailable</h1>\n"
                    "<p>The antivirus scanner is temporarily unavailable.</p>\n"
                    "<p>Please try again later.</p>\n"
                    "</body></html>\n";

                ci_membuf_write(data->error_page, error_html,
                               strlen(error_html), 0);
            }

            /* Modify response to 403 */
            ci_http_response_reset_headers(req);
            ci_http_response_create(req, 1, 1);
            ci_http_response_add_header(req, "HTTP/1.1 403 Forbidden");
            ci_http_response_add_header(req, "Content-Type: text/html");
            ci_http_response_add_header(req, "Connection: close");

            data->virus_found = 1;  /* Treat as virus to use error page path */
            return CI_MOD_DONE;
        }
    }

    /* ---------------------------------------------------------- */
    /* ClamAV scan passed (clean) — proceed to OTT scan            */
    /* ---------------------------------------------------------- */
    ci_debug_printf(4, "sentinel_resp: ClamAV scan clean\n");

    /* ========================================================================== */
    /* STEP 2: Check if host is in allowlist                                     */
    /* ========================================================================== */

    if (!is_allowed_domain(data->host)) {
        /* ---------------------------------------------------------- */
        /* Host NOT in allowlist — pass through without OTT scan      */
        /* ---------------------------------------------------------- */
        ci_debug_printf(4, "sentinel_resp: "
            "Host '%s' not in allowlist — passing through\n",
            data->host);
        return ci_req_allow204(req) ? CI_MOD_ALLOW204 : CI_MOD_DONE;
    }

    ci_debug_printf(4, "sentinel_resp: "
        "Host '%s' in allowlist — proceeding with OTT scan\n",
        data->host);

    /* ========================================================================== */
    /* STEP 3: Decompress if gzip                                                */
    /* ========================================================================== */

    if (data->is_gzip) {
        int decomp_ret;

        ci_debug_printf(4, "sentinel_resp: "
            "Decompressing gzip body (%zu bytes)\n", body_len);

        decomp_ret = decompress_gzip(body_raw, body_len,
                                     &decompressed, &decompressed_len);

        if (decomp_ret == -2) {
            /* ---------------------------------------------------------- */
            /* Decompression bomb detected — skip OTT scan, pass through  */
            /* ---------------------------------------------------------- */
            ci_debug_printf(1, "sentinel_resp: WARNING: "
                "Decompression bomb detected — "
                "skipping OTT scan, passing original body\n");
            return ci_req_allow204(req) ? CI_MOD_ALLOW204 : CI_MOD_DONE;

        } else if (decomp_ret == -1) {
            /* ---------------------------------------------------------- */
            /* Decompression failed — skip OTT scan, pass through         */
            /* ---------------------------------------------------------- */
            ci_debug_printf(2, "sentinel_resp: WARNING: "
                "Decompression failed — "
                "skipping OTT scan, passing original body\n");
            return ci_req_allow204(req) ? CI_MOD_ALLOW204 : CI_MOD_DONE;
        }

        /* Decompression successful */
        ci_debug_printf(4, "sentinel_resp: "
            "Decompressed %zu → %zu bytes\n",
            body_len, decompressed_len);

        scan_buf = decompressed;
        scan_len = decompressed_len;
        was_decompressed = 1;

    } else {
        /* Not gzip — scan original body */
        scan_buf = body_raw;
        scan_len = body_len;
    }

    /* ========================================================================== */
    /* STEP 4: Scan for OTT codes and process approvals                          */
    /* ========================================================================== */

    ci_debug_printf(4, "sentinel_resp: "
        "Scanning for OTT codes (%zu bytes)\n", scan_len);

    /* Create a mutable copy of the scan buffer for OTT stripping */
    char *mutable_buf = malloc(scan_len + 1);
    if (!mutable_buf) {
        ci_debug_printf(1, "sentinel_resp: ERROR: "
            "Failed to allocate mutable buffer for OTT scan\n");
        if (decompressed) free(decompressed);
        return CI_ERROR;
    }
    memcpy(mutable_buf, scan_buf, scan_len);
    mutable_buf[scan_len] = '\0';

    /* Scan for OTT codes using regex */
    {
        size_t offset = 0;
        while (offset < scan_len) {
            const char *search_start = mutable_buf + offset;

            if (regexec(&ott_regex, search_start, 1, &match, 0) == 0) {
                /* OTT code found */
                char ott_code[16];
                size_t ott_len = match.rm_eo - match.rm_so;

                if (ott_len >= sizeof(ott_code)) {
                    ci_debug_printf(2, "sentinel_resp: WARNING: "
                        "OTT code too long (%zu bytes) — skipping\n",
                        ott_len);
                    offset += match.rm_eo;
                    continue;
                }

                /* Extract OTT code */
                memcpy(ott_code, search_start + match.rm_so, ott_len);
                ott_code[ott_len] = '\0';

                ci_debug_printf(3, "sentinel_resp: "
                    "Found OTT code: %s\n", ott_code);

                /* Process approval (returns 1=success, 0=rejected, -1=error) */
                int approval_ret = process_ott_approval(ott_code, data->host);
                if (approval_ret == 1) {
                    ci_debug_printf(3, "sentinel_resp: "
                        "OTT approval successful: %s\n", ott_code);
                    ott_count++;
                } else {
                    ci_debug_printf(3, "sentinel_resp: "
                        "OTT approval %s: %s\n",
                        approval_ret == -1 ? "error" : "rejected",
                        ott_code);
                }

                /* Strip OTT code (replace with asterisks) */
                memset(mutable_buf + offset + match.rm_so, '*', ott_len);

                /* Move past this OTT code */
                offset += match.rm_eo;
            } else {
                /* No more OTT codes found */
                break;
            }
        }
    }

    ci_debug_printf(3, "sentinel_resp: "
        "OTT scan complete — processed %d code(s)\n", ott_count);

    /* ========================================================================== */
    /* STEP 5: Recompress if was gzip                                            */
    /* ========================================================================== */

    if (was_decompressed && ott_count > 0) {
        /* OTT codes were stripped — need to recompress */
        char *recompressed = NULL;
        size_t recompressed_len = 0;
        int comp_ret;

        ci_debug_printf(4, "sentinel_resp: "
            "Recompressing modified body (%zu bytes)\n", scan_len);

        comp_ret = compress_gzip(mutable_buf, scan_len,
                                &recompressed, &recompressed_len);

        if (comp_ret == 0) {
            /* Recompression successful — replace body */
            ci_debug_printf(4, "sentinel_resp: "
                "Recompressed %zu → %zu bytes\n",
                scan_len, recompressed_len);

            /* Replace body membuf with recompressed data */
            ci_membuf_free(data->body);
            data->body = ci_membuf_new_sized(recompressed_len + 1024);
            if (data->body) {
                ci_membuf_write(data->body, recompressed,
                               recompressed_len, 0);
                data->total_body_len = recompressed_len;

                /* Update cached file with recompressed data */
                ci_cached_file_destroy(data->cached);
                data->cached = ci_cached_file_new(0);
                if (data->cached) {
                    ci_cached_file_write(data->cached, recompressed,
                                        recompressed_len, 0);
                }
            }

            free(recompressed);
        } else {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Recompression failed — passing original body\n");
        }

    } else if (!was_decompressed && ott_count > 0) {
        /* Body was not gzip, but OTT codes were stripped */
        ci_debug_printf(4, "sentinel_resp: "
            "Updating body with stripped OTT codes\n");

        /* Replace body membuf with modified data */
        ci_membuf_free(data->body);
        data->body = ci_membuf_new_sized(scan_len + 1024);
        if (data->body) {
            ci_membuf_write(data->body, mutable_buf, scan_len, 0);
            data->total_body_len = scan_len;

            /* Update cached file with modified data */
            ci_cached_file_destroy(data->cached);
            data->cached = ci_cached_file_new(0);
            if (data->cached) {
                ci_cached_file_write(data->cached, mutable_buf,
                                    scan_len, 0);
            }
        }
    }

    /* ---------------------------------------------------------- */
    /* Clean up                                                    */
    /* ---------------------------------------------------------- */
    free(mutable_buf);
    if (decompressed) {
        free(decompressed);
    }

    ci_debug_printf(3, "sentinel_resp: "
        "Processing complete — passing through\n");

    return (ott_count > 0) ? CI_MOD_DONE
         : ci_req_allow204(req) ? CI_MOD_ALLOW204 : CI_MOD_DONE;
}

/* ========================================================================== */
/* Request Processing - I/O Callback                                          */
/* ========================================================================== */

/**
 * sentinel_resp_io() - Body accumulation and write-back
 *
 * Called repeatedly to transfer body data between client and server.
 * Implements bidirectional data flow:
 *
 * READ PATH (rbuf != NULL):
 *   - Accumulate body chunks into ci_membuf_t (up to MAX_BODY_SIZE)
 *   - Write all chunks to ci_cached_file_t for pass-through
 *   - Set eof flag when read returns CI_EOF
 *
 * WRITE PATH (wbuf != NULL):
 *   - After processing (eof set), stream from modified body or cached file
 *   - Handle error page streaming for virus blocks
 *
 * @param rbuf:  Read buffer (data from server)
 * @param rlen:  Read buffer length (input: available, output: consumed)
 * @param wbuf:  Write buffer (data to client)
 * @param wlen:  Write buffer length (input: available, output: written)
 * @param iseof: End-of-file flag from c-ICAP
 * @param req:   ICAP request context
 *
 * Returns: CI_OK on success, CI_ERROR on failure
 *
 * Validates: Requirements 2.1, 2.11
 */
static int sentinel_resp_io(char *wbuf, int *wlen, char *rbuf, int *rlen,
                           int iseof, ci_request_t *req)
{
    sentinel_resp_data_t *data;
    int ret;

    /* ---------------------------------------------------------- */
    /* Get per-request data                                       */
    /* ---------------------------------------------------------- */
    data = ci_service_data(req);
    if (!data) {
        ci_debug_printf(1, "sentinel_resp: ERROR: "
            "No request data in sentinel_resp_io\n");
        return CI_ERROR;
    }

    /* ---------------------------------------------------------- */
    /* READ PATH: Accumulate body chunks from server              */
    /* ---------------------------------------------------------- */
    if (rbuf && rlen && *rlen > 0) {
        /* ---------------------------------------------------------- */
        /* Allocate body membuf on first read                         */
        /* ---------------------------------------------------------- */
        if (!data->body) {
            data->body = ci_membuf_new_sized(MAX_BODY_SIZE);
            if (!data->body) {
                ci_debug_printf(1, "sentinel_resp: ERROR: "
                    "Failed to allocate body membuf\n");
                return CI_ERROR;
            }
            ci_debug_printf(5, "sentinel_resp: "
                "Allocated body membuf (max %d bytes)\n",
                MAX_BODY_SIZE);
        }

        /* ---------------------------------------------------------- */
        /* Allocate cached file on first read (for pass-through)      */
        /* ---------------------------------------------------------- */
        if (!data->cached) {
            data->cached = ci_cached_file_new(0);  /* 0 = use default size */
            if (!data->cached) {
                ci_debug_printf(1, "sentinel_resp: ERROR: "
                    "Failed to allocate cached file\n");
                return CI_ERROR;
            }
            ci_debug_printf(5, "sentinel_resp: "
                "Allocated cached file for pass-through\n");
        }

        /* ---------------------------------------------------------- */
        /* Write chunk to body membuf (up to MAX_BODY_SIZE)           */
        /* ---------------------------------------------------------- */
        if (data->total_body_len < MAX_BODY_SIZE) {
            size_t to_write = *rlen;

            /* Limit write to MAX_BODY_SIZE */
            if (data->total_body_len + to_write > MAX_BODY_SIZE) {
                to_write = MAX_BODY_SIZE - data->total_body_len;
                ci_debug_printf(3, "sentinel_resp: "
                    "Body size limit reached — "
                    "truncating accumulation at %d bytes\n",
                    MAX_BODY_SIZE);
            }

            if (to_write > 0) {
                ret = ci_membuf_write(data->body, rbuf, to_write, 0);
                if (ret < 0) {
                    ci_debug_printf(1, "sentinel_resp: ERROR: "
                        "Failed to write to body membuf\n");
                    return CI_ERROR;
                }
                ci_debug_printf(5, "sentinel_resp: "
                    "Wrote %zu bytes to body membuf "
                    "(total: %zu)\n",
                    to_write, data->total_body_len + to_write);
            }
        }

        /* ---------------------------------------------------------- */
        /* Write chunk to cached file (all data, for pass-through)    */
        /* ---------------------------------------------------------- */
        ret = ci_cached_file_write(data->cached, rbuf, *rlen, 0);
        if (ret < 0) {
            ci_debug_printf(1, "sentinel_resp: ERROR: "
                "Failed to write to cached file\n");
            return CI_ERROR;
        }

        data->total_body_len += *rlen;
        ci_debug_printf(5, "sentinel_resp: "
            "Wrote %d bytes to cached file (total: %zu)\n",
            *rlen, data->total_body_len);
    }

    /* ---------------------------------------------------------- */
    /* Handle EOF from server                                      */
    /* ---------------------------------------------------------- */
    if (iseof) {
        if (!data->eof) {
            data->eof = 1;
            ci_debug_printf(4, "sentinel_resp: "
                "EOF received — total body: %zu bytes\n",
                data->total_body_len);

            /* Unlock request data for processing */
            ci_req_unlock_data(req);
        }
    }

    /* ---------------------------------------------------------- */
    /* WRITE PATH: Stream data back to client after processing    */
    /* ---------------------------------------------------------- */
    if (wbuf && wlen && *wlen > 0) {
        /* Only send data back AFTER processing is complete (eof set) */
        if (!data->eof) {
            *wlen = 0;
            return CI_OK;
        }

        /* ---------------------------------------------------------- */
        /* Case 1: Virus found — stream error page                    */
        /* ---------------------------------------------------------- */
        if (data->virus_found && data->error_page) {
            size_t error_page_size = ci_membuf_size(data->error_page);
            size_t remaining = error_page_size - data->error_page_sent;

            if (remaining > 0) {
                size_t to_send = (remaining < (size_t)*wlen) ?
                                 remaining : (size_t)*wlen;
                const char *error_data =
                    (const char *)ci_membuf_raw(data->error_page);

                memcpy(wbuf, error_data + data->error_page_sent, to_send);
                data->error_page_sent += to_send;
                *wlen = to_send;

                ci_debug_printf(5, "sentinel_resp: "
                    "Sent %zu bytes of error page "
                    "(%zu/%zu)\n",
                    to_send, data->error_page_sent,
                    error_page_size);
            } else {
                /* Error page fully sent */
                *wlen = CI_EOF;
                ci_debug_printf(4, "sentinel_resp: "
                    "Error page fully sent\n");
            }

            return CI_OK;
        }

        /* ---------------------------------------------------------- */
        /* Case 2: Normal pass-through — stream from cached file      */
        /* ---------------------------------------------------------- */
        if (data->cached) {
            ret = ci_cached_file_read(data->cached, wbuf, *wlen);
            if (ret > 0) {
                *wlen = ret;
                ci_debug_printf(5, "sentinel_resp: "
                    "Sent %d bytes from cached file\n", ret);
            } else if (ret == 0) {
                /* End of cached file */
                *wlen = CI_EOF;
                ci_debug_printf(4, "sentinel_resp: "
                    "Cached file fully sent\n");
            } else {
                /* Read error */
                ci_debug_printf(1, "sentinel_resp: ERROR: "
                    "Failed to read from cached file\n");
                *wlen = CI_ERROR;
                return CI_ERROR;
            }

            return CI_OK;
        }

        /* ---------------------------------------------------------- */
        /* Case 3: No data to send (shouldn't happen)                 */
        /* ---------------------------------------------------------- */
        ci_debug_printf(2, "sentinel_resp: WARNING: "
            "No data source for write path\n");
        *wlen = CI_EOF;
    }

    return CI_OK;
}
