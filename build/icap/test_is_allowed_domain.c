/*
 * test_is_allowed_domain.c — Unit tests for dot-boundary domain matching
 *
 * Standalone test harness that exercises is_allowed_domain() logic
 * without requiring c-ICAP headers. We replicate the function and
 * its static data structures here for isolated testing.
 *
 * Validates: Requirements 2.2, 2.3
 *
 * Compile: gcc -o test_is_allowed_domain test_is_allowed_domain.c
 * Run:     ./test_is_allowed_domain
 */

#include <stdio.h>
#include <string.h>
#include <strings.h>   /* strcasecmp */
#include <stdlib.h>

/* Replicate the static data structures from srv_molis_approval.c */
#define MAX_DOMAINS 16

static char allowed_domains[MAX_DOMAINS][256];
static int  domain_count = 0;

/* ---------- Function under test (copied from srv_molis_approval.c) ---------- */

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
            const char *bare = entry + 1;
            size_t bare_len = entry_len - 1;

            if (host_len == bare_len &&
                strcasecmp(host, bare) == 0) {
                return 1;
            }

            if (host_len > entry_len) {
                const char *suffix = host + (host_len - entry_len);
                if (strcasecmp(suffix, entry) == 0) {
                    return 1;
                }
            }
        } else {
            if (strcasecmp(host, entry) == 0) {
                return 1;
            }
        }
    }

    return 0;
}

/* ---------- Test helpers ---------- */

static int tests_run = 0;
static int tests_passed = 0;
static int tests_failed = 0;

static void assert_allowed(const char *host, int expected, const char *desc)
{
    int result = is_allowed_domain(host);
    tests_run++;
    if (result == expected) {
        tests_passed++;
        printf("  PASS: %s\n", desc);
    } else {
        tests_failed++;
        printf("  FAIL: %s (expected %d, got %d)\n", desc, expected, result);
    }
}

static void setup_domains(const char **domains, int count)
{
    int i;
    domain_count = 0;
    for (i = 0; i < count && i < MAX_DOMAINS; i++) {
        strncpy(allowed_domains[i], domains[i], 255);
        allowed_domains[i][255] = '\0';
        domain_count++;
    }
}

/* ---------- Test cases ---------- */

static void test_dot_prefixed_suffix_match(void)
{
    const char *domains[] = { ".slack.com" };
    setup_domains(domains, 1);

    printf("\n[Dot-prefixed suffix match]\n");
    assert_allowed("api.slack.com", 1,
        "api.slack.com matches .slack.com (subdomain)");
    assert_allowed("deep.api.slack.com", 1,
        "deep.api.slack.com matches .slack.com (deep subdomain)");
    assert_allowed("a.b.c.slack.com", 1,
        "a.b.c.slack.com matches .slack.com (multi-level)");
}

static void test_dot_boundary_enforcement(void)
{
    const char *domains[] = { ".slack.com" };
    setup_domains(domains, 1);

    printf("\n[Dot-boundary enforcement — CWE-346]\n");
    assert_allowed("evil-slack.com", 0,
        "evil-slack.com does NOT match .slack.com (no dot boundary)");
    assert_allowed("notslack.com", 0,
        "notslack.com does NOT match .slack.com");
    assert_allowed("fakeslack.com", 0,
        "fakeslack.com does NOT match .slack.com");
    assert_allowed("xslack.com", 0,
        "xslack.com does NOT match .slack.com");
}

static void test_exact_domain_without_dot(void)
{
    const char *domains[] = { ".slack.com" };
    setup_domains(domains, 1);

    printf("\n[Exact domain without leading dot]\n");
    assert_allowed("slack.com", 1,
        "slack.com matches .slack.com (bare domain)");
}

static void test_non_dot_prefixed_exact_match(void)
{
    const char *domains[] = { "exact.example.com" };
    setup_domains(domains, 1);

    printf("\n[Non-dot-prefixed exact match]\n");
    assert_allowed("exact.example.com", 1,
        "exact.example.com matches exact.example.com");
    assert_allowed("sub.exact.example.com", 0,
        "sub.exact.example.com does NOT match (not a suffix rule)");
    assert_allowed("example.com", 0,
        "example.com does NOT match exact.example.com");
}

static void test_case_insensitive(void)
{
    const char *domains[] = { ".Slack.COM" };
    setup_domains(domains, 1);

    printf("\n[Case-insensitive matching]\n");
    assert_allowed("api.slack.com", 1,
        "api.slack.com matches .Slack.COM (case-insensitive suffix)");
    assert_allowed("SLACK.COM", 1,
        "SLACK.COM matches .Slack.COM (case-insensitive bare)");
    assert_allowed("Api.SLACK.Com", 1,
        "Api.SLACK.Com matches .Slack.COM (mixed case)");
}

static void test_null_and_empty(void)
{
    const char *domains[] = { ".slack.com" };
    setup_domains(domains, 1);

    printf("\n[NULL and empty inputs]\n");
    assert_allowed(NULL, 0, "NULL host returns 0");
    assert_allowed("", 0, "empty host returns 0");
}

static void test_no_domains_configured(void)
{
    printf("\n[No domains configured]\n");
    domain_count = 0;
    assert_allowed("api.slack.com", 0,
        "No domains configured — always returns 0");
}

static void test_default_domains(void)
{
    const char *domains[] = {
        ".api.telegram.org",
        ".api.slack.com",
        ".discord.com"
    };
    setup_domains(domains, 3);

    printf("\n[Default domain allowlist]\n");
    assert_allowed("api.telegram.org", 1,
        "api.telegram.org matches .api.telegram.org (bare)");
    assert_allowed("bot.api.telegram.org", 1,
        "bot.api.telegram.org matches .api.telegram.org (sub)");
    assert_allowed("api.slack.com", 1,
        "api.slack.com matches .api.slack.com (bare)");
    assert_allowed("xoxb.api.slack.com", 1,
        "xoxb.api.slack.com matches .api.slack.com (sub)");
    assert_allowed("discord.com", 1,
        "discord.com matches .discord.com (bare)");
    assert_allowed("cdn.discord.com", 1,
        "cdn.discord.com matches .discord.com (sub)");
    assert_allowed("evil.com", 0,
        "evil.com does NOT match any default domain");
    assert_allowed("evil-discord.com", 0,
        "evil-discord.com does NOT match .discord.com");
}

int main(void)
{
    printf("=== is_allowed_domain() unit tests ===\n");

    test_dot_prefixed_suffix_match();
    test_dot_boundary_enforcement();
    test_exact_domain_without_dot();
    test_non_dot_prefixed_exact_match();
    test_case_insensitive();
    test_null_and_empty();
    test_no_domains_configured();
    test_default_domains();

    printf("\n=== Results: %d/%d passed, %d failed ===\n",
           tests_passed, tests_run, tests_failed);

    return tests_failed > 0 ? 1 : 0;
}
