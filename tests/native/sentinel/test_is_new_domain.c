/*
 * test_is_new_domain.c - Unit tests for dot-boundary domain matching in DLP
 *
 * Standalone test harness that exercises is_new_domain() logic
 * from srv_polis_dlp.c without requiring c-ICAP headers.
 *
 * Validates: Requirement 3.1, 3.2, 3.3, 3.4, 3.5, 3.6
 *
 * Compile: gcc -o test_is_new_domain test_is_new_domain.c
 * Run:     ./test_is_new_domain
 */

#include <stdio.h>
#include <string.h>
#include <strings.h>
#include <stdlib.h>

/* Function under test (copied from srv_polis_dlp.c) */
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

        if (hlen >= dlen &&
            strcasecmp(host + (hlen - dlen),
                       known_domains[i]) == 0) {
            return 0;
        }

        if (strcasecmp(host, known_domains[i] + 1) == 0) {
            return 0;
        }
    }

    return 1;
}

static int tests_run = 0;
static int tests_passed = 0;
static int tests_failed = 0;

static void assert_new(const char *host, int expected, const char *desc)
{
    int result = is_new_domain(host);
    tests_run++;
    if (result == expected) {
        tests_passed++;
        printf("  PASS: %s\n", desc);
    } else {
        tests_failed++;
        printf("  FAIL: %s (expected %d, got %d)\n", desc, expected, result);
    }
}

static void test_known_domains(void)
{
    printf("\n[Known Domains - Should return 0]\n");
    assert_new("api.anthropic.com", 0, "api.anthropic.com is known");
    assert_new("api.openai.com", 0, "api.openai.com is known");
    assert_new("api.github.com", 0, "api.github.com is known");
    assert_new("github.com", 0, "github.com is known (exact match)");
    assert_new("s3.amazonaws.com", 0, "s3.amazonaws.com is known (suffix)");
}

static void test_dot_boundary(void)
{
    printf("\n[Dot-Boundary Enforcement - Requirement 3.4]\n");
    assert_new("evil-github.com", 1, "evil-github.com is NEW (no dot boundary)");
    assert_new("my-api.github.com", 0, "my-api.github.com is known (subdomain of .github.com)");
    assert_new("attacker.api.github.com.io", 1, "api.github.com as prefix is NEW");
}

static void test_case_insensitivity(void)
{
    printf("\n[Case Insensitivity - Requirement 3.6]\n");
    assert_new("API.GITHUB.COM", 0, "Uppercase is known");
    assert_new("Github.Com", 0, "Mixed case is known");
}

static void test_edge_cases(void)
{
    printf("\n[Edge Cases]\n");
    assert_new(NULL, 1, "NULL is NEW");
    assert_new("", 1, "Empty string is NEW");
    assert_new("google.com", 1, "google.com is NEW");
}

int main(void)
{
    printf("=== is_new_domain() unit tests ===\n");
    test_known_domains();
    test_dot_boundary();
    test_case_insensitivity();
    test_edge_cases();
    printf("\n=== Results: %d/%d passed, %d failed ===\n", tests_passed, tests_run, tests_failed);
    return tests_failed > 0 ? 1 : 0;
}