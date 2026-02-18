# Unit Tests for `get_status_phrase()` Covering All Status Codes

**Category:** Testing
**Complexity:** 1/10
**Necessity:** 4/10
**Status:** Planning

---

## Overview

Add comprehensive unit tests to `src/models/http_status_codes.rs` that cover every status code handled by `get_status_phrase()`. The function currently maps 57 HTTP status codes to their standard phrases. There are no existing tests for this function.

**Goal:** Ensure every status code returns the correct phrase per RFC 7231/9110, and that unknown codes return an empty string.

---

## Current State

### `get_status_phrase()` (src/models/http_status_codes.rs)

The function is a single `match` expression covering:
- 1xx Informational: 100, 101, 102, 103
- 2xx Success: 200-208, 226
- 3xx Redirection: 300-308
- 4xx Client Error: 400-418, 421-426, 428, 429, 431, 451
- 5xx Server Error: 500-508, 510, 511
- Default: `_ => String::from("")`

The function returns `String` (not `&str`), allocating on every call.

### No Existing Tests

There are no `#[cfg(test)]` modules in `http_status_codes.rs`.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/models/http_status_codes.rs`

**Add:** `#[cfg(test)]` module with exhaustive tests.

---

## Step-by-Step Implementation

### Step 1: Add Test Module

Add at the bottom of `src/models/http_status_codes.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // --- 1xx Informational ---

    #[test]
    fn status_100_continue() {
        assert_eq!(get_status_phrase(100), "Continue");
    }

    #[test]
    fn status_101_switching_protocols() {
        assert_eq!(get_status_phrase(101), "Switching Protocols");
    }

    #[test]
    fn status_102_processing() {
        assert_eq!(get_status_phrase(102), "Processing");
    }

    #[test]
    fn status_103_early_hints() {
        assert_eq!(get_status_phrase(103), "Early Hints");
    }

    // --- 2xx Success ---

    #[test]
    fn status_200_ok() {
        assert_eq!(get_status_phrase(200), "OK");
    }

    #[test]
    fn status_201_created() {
        assert_eq!(get_status_phrase(201), "Created");
    }

    #[test]
    fn status_202_accepted() {
        assert_eq!(get_status_phrase(202), "Accepted");
    }

    #[test]
    fn status_203_non_authoritative() {
        assert_eq!(get_status_phrase(203), "Non-Authoritative Information");
    }

    #[test]
    fn status_204_no_content() {
        assert_eq!(get_status_phrase(204), "No Content");
    }

    #[test]
    fn status_205_reset_content() {
        assert_eq!(get_status_phrase(205), "Reset Content");
    }

    #[test]
    fn status_206_partial_content() {
        assert_eq!(get_status_phrase(206), "Partial Content");
    }

    #[test]
    fn status_207_multi_status() {
        assert_eq!(get_status_phrase(207), "Multi-Status");
    }

    #[test]
    fn status_208_already_reported() {
        assert_eq!(get_status_phrase(208), "Already Reported");
    }

    #[test]
    fn status_226_im_used() {
        assert_eq!(get_status_phrase(226), "IM Used");
    }

    // --- 3xx Redirection ---

    #[test]
    fn status_300_multiple_choices() {
        assert_eq!(get_status_phrase(300), "Multiple Choices");
    }

    #[test]
    fn status_301_moved_permanently() {
        assert_eq!(get_status_phrase(301), "Moved Permanently");
    }

    #[test]
    fn status_302_found() {
        assert_eq!(get_status_phrase(302), "Found");
    }

    #[test]
    fn status_303_see_other() {
        assert_eq!(get_status_phrase(303), "See Other");
    }

    #[test]
    fn status_304_not_modified() {
        assert_eq!(get_status_phrase(304), "Not Modified");
    }

    #[test]
    fn status_305_use_proxy() {
        assert_eq!(get_status_phrase(305), "Use Proxy");
    }

    #[test]
    fn status_306_unused() {
        assert_eq!(get_status_phrase(306), "Unused");
    }

    #[test]
    fn status_307_temporary_redirect() {
        assert_eq!(get_status_phrase(307), "Temporary Redirect");
    }

    #[test]
    fn status_308_permanent_redirect() {
        assert_eq!(get_status_phrase(308), "Permanent Redirect");
    }

    // --- 4xx Client Error ---

    #[test]
    fn status_400_bad_request() {
        assert_eq!(get_status_phrase(400), "Bad Request");
    }

    #[test]
    fn status_401_unauthorized() {
        assert_eq!(get_status_phrase(401), "Unauthorized");
    }

    #[test]
    fn status_402_payment_required() {
        assert_eq!(get_status_phrase(402), "Payment Required");
    }

    #[test]
    fn status_403_forbidden() {
        assert_eq!(get_status_phrase(403), "Forbidden");
    }

    #[test]
    fn status_404_not_found() {
        assert_eq!(get_status_phrase(404), "Not Found");
    }

    #[test]
    fn status_405_method_not_allowed() {
        assert_eq!(get_status_phrase(405), "Method Not Allowed");
    }

    #[test]
    fn status_406_not_acceptable() {
        assert_eq!(get_status_phrase(406), "Not Acceptable");
    }

    #[test]
    fn status_407_proxy_auth_required() {
        assert_eq!(get_status_phrase(407), "Proxy Authentication Required");
    }

    #[test]
    fn status_408_request_timeout() {
        assert_eq!(get_status_phrase(408), "Request Timeout");
    }

    #[test]
    fn status_409_conflict() {
        assert_eq!(get_status_phrase(409), "Conflict");
    }

    #[test]
    fn status_410_gone() {
        assert_eq!(get_status_phrase(410), "Gone");
    }

    #[test]
    fn status_411_length_required() {
        assert_eq!(get_status_phrase(411), "Length Required");
    }

    #[test]
    fn status_412_precondition_failed() {
        assert_eq!(get_status_phrase(412), "Precondition Failed");
    }

    #[test]
    fn status_413_content_too_large() {
        assert_eq!(get_status_phrase(413), "Content Too Large");
    }

    #[test]
    fn status_414_uri_too_long() {
        assert_eq!(get_status_phrase(414), "URI Too Long");
    }

    #[test]
    fn status_415_unsupported_media_type() {
        assert_eq!(get_status_phrase(415), "Unsupported Media Type");
    }

    #[test]
    fn status_416_range_not_satisfiable() {
        assert_eq!(get_status_phrase(416), "Range Not Satisfiable");
    }

    #[test]
    fn status_417_expectation_failed() {
        assert_eq!(get_status_phrase(417), "Expectation Failed");
    }

    #[test]
    fn status_418_teapot() {
        assert_eq!(get_status_phrase(418), "I'm a teapot");
    }

    #[test]
    fn status_421_misdirected_request() {
        assert_eq!(get_status_phrase(421), "Misdirected Request");
    }

    #[test]
    fn status_422_unprocessable_content() {
        assert_eq!(get_status_phrase(422), "Unprocessable Content");
    }

    #[test]
    fn status_423_locked() {
        assert_eq!(get_status_phrase(423), "Locked");
    }

    #[test]
    fn status_424_failed_dependency() {
        assert_eq!(get_status_phrase(424), "Failed Dependency");
    }

    #[test]
    fn status_425_too_early() {
        assert_eq!(get_status_phrase(425), "Too Early");
    }

    #[test]
    fn status_426_upgrade_required() {
        assert_eq!(get_status_phrase(426), "Upgrade Required");
    }

    #[test]
    fn status_428_precondition_required() {
        assert_eq!(get_status_phrase(428), "Precondition Required");
    }

    #[test]
    fn status_429_too_many_requests() {
        assert_eq!(get_status_phrase(429), "Too Many Requests");
    }

    #[test]
    fn status_431_header_fields_too_large() {
        assert_eq!(get_status_phrase(431), "Request Header Fields Too Large");
    }

    #[test]
    fn status_451_unavailable_for_legal_reasons() {
        assert_eq!(get_status_phrase(451), "Unavailable For Legal Reasons");
    }

    // --- 5xx Server Error ---

    #[test]
    fn status_500_internal_server_error() {
        assert_eq!(get_status_phrase(500), "Internal Server Error");
    }

    #[test]
    fn status_501_not_implemented() {
        assert_eq!(get_status_phrase(501), "Not Implemented");
    }

    #[test]
    fn status_502_bad_gateway() {
        assert_eq!(get_status_phrase(502), "Bad Gateway");
    }

    #[test]
    fn status_503_service_unavailable() {
        assert_eq!(get_status_phrase(503), "Service Unavailable");
    }

    #[test]
    fn status_504_gateway_timeout() {
        assert_eq!(get_status_phrase(504), "Gateway Timeout");
    }

    #[test]
    fn status_505_http_version_not_supported() {
        assert_eq!(get_status_phrase(505), "HTTP Version Not Supported");
    }

    #[test]
    fn status_506_variant_also_negotiates() {
        assert_eq!(get_status_phrase(506), "Variant Also Negotiates");
    }

    #[test]
    fn status_507_insufficient_storage() {
        assert_eq!(get_status_phrase(507), "Insufficient Storage");
    }

    #[test]
    fn status_508_loop_detected() {
        assert_eq!(get_status_phrase(508), "Loop Detected");
    }

    #[test]
    fn status_510_not_extended() {
        assert_eq!(get_status_phrase(510), "Not Extended");
    }

    #[test]
    fn status_511_network_auth_required() {
        assert_eq!(get_status_phrase(511), "Network Authentication Required");
    }

    // --- Unknown / Default ---

    #[test]
    fn status_unknown_returns_empty() {
        assert_eq!(get_status_phrase(0), "");
        assert_eq!(get_status_phrase(1), "");
        assert_eq!(get_status_phrase(99), "");
        assert_eq!(get_status_phrase(199), "");
        assert_eq!(get_status_phrase(299), "");
        assert_eq!(get_status_phrase(399), "");
        assert_eq!(get_status_phrase(499), "");
        assert_eq!(get_status_phrase(599), "");
        assert_eq!(get_status_phrase(999), "");
        assert_eq!(get_status_phrase(65535), "");
    }

    // --- Gap codes (valid range but not defined) ---

    #[test]
    fn status_gap_codes_return_empty() {
        // Codes between defined ones that have no phrase
        assert_eq!(get_status_phrase(104), "");  // Between 103 and 200
        assert_eq!(get_status_phrase(209), "");  // Between 208 and 226
        assert_eq!(get_status_phrase(227), "");  // Between 226 and 300
        assert_eq!(get_status_phrase(309), "");  // Between 308 and 400
        assert_eq!(get_status_phrase(419), "");  // Between 418 and 421
        assert_eq!(get_status_phrase(420), "");  // Between 418 and 421
        assert_eq!(get_status_phrase(427), "");  // Between 426 and 428
        assert_eq!(get_status_phrase(430), "");  // Between 429 and 431
        assert_eq!(get_status_phrase(450), "");  // Between 431 and 451
        assert_eq!(get_status_phrase(509), "");  // Between 508 and 510
    }
}
```

---

## Edge Cases & Considerations

### 1. RFC Compliance

**Standard:** The phrase strings should match RFC 9110 (HTTP Semantics, which supersedes RFC 7231).

**Noted discrepancy:** Code 413 uses "Content Too Large" (RFC 9110 name) rather than the older "Payload Too Large" (RFC 7231 name). This is correct for modern compliance.

### 2. Empty String for Unknown Codes

**Behavior:** Unknown codes return `""`. Callers should handle this — an empty reason phrase is valid in HTTP (the phrase is optional).

**Test:** Multiple unknown codes are tested including 0, edge values, and gap codes.

### 3. Test Count

This adds approximately 67 tests. With the existing 34 tests, the total becomes ~101 tests. All are trivial `assert_eq!` comparisons and run in microseconds.

---

## Testing Strategy

### Running the Tests

```bash
cargo test http_status_codes
```

Or run all tests:

```bash
cargo test
```

### Expected Results

All 67 new tests pass. No regressions in existing tests.

---

## Implementation Checklist

- [ ] Add `#[cfg(test)] mod tests` to `src/models/http_status_codes.rs`
- [ ] Add tests for all 1xx codes (4 tests)
- [ ] Add tests for all 2xx codes (10 tests)
- [ ] Add tests for all 3xx codes (9 tests)
- [ ] Add tests for all 4xx codes (24 tests)
- [ ] Add tests for all 5xx codes (11 tests)
- [ ] Add tests for unknown codes (2 tests)
- [ ] Run `cargo test` — all tests pass

---

## Dependencies

- **No dependencies** — pure unit tests on an existing function

---

## References

- [RFC 9110 - HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110)
- [IANA HTTP Status Code Registry](https://www.iana.org/assignments/http-status-codes/http-status-codes.xhtml)
