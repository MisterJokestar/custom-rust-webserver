# Unit Tests for `clean_route()` with Adversarial Inputs

**Category:** Testing
**Complexity:** 2/10
**Necessity:** 7/10
**Status:** Planning

---

## Overview

Add comprehensive unit tests for the `clean_route()` function in `src/main.rs` that cover adversarial and edge-case inputs. This function is a critical security component — it sanitizes request URIs before route lookup. Malicious inputs could potentially bypass the sanitization and access unintended routes.

**Goal:** Validate that `clean_route()` correctly handles path traversal attempts, encoded characters, edge cases (empty input, single slashes, double slashes), and adversarial patterns.

---

## Current State

### `clean_route()` (src/main.rs, lines 77-89)

```rust
fn clean_route(route: &String) -> String {
    let mut clean_route = String::from("");
    for part in route.split("/").collect::<Vec<_>>() {
        if part == "" || part == "." || part == ".." {
            continue;
        }
        clean_route.push_str(format!("/{part}").as_str());
    }
    if clean_route == "" {
        clean_route = String::from("/");
    }
    clean_route
}
```

### Behavior

The function:
1. Splits the route on `/`
2. Skips empty segments (`""`), single dot (`.`), and double dot (`..`)
3. Prepends `/` to remaining segments
4. Returns `/` if no segments remain

### Security Role

This is the first line of defense against path traversal. It runs before the route hashmap lookup, so it determines what key is used for matching. If this function doesn't properly sanitize, an attacker could potentially access routes they shouldn't.

### Current Test Coverage

There are no existing tests for `clean_route()`. It's a private function in `src/main.rs`, so tests must be in the same file within a `#[cfg(test)]` module.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Add:** `#[cfg(test)]` module with `clean_route()` tests.

---

## Step-by-Step Implementation

### Step 1: Add Test Module

Add at the bottom of `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // --- Basic functionality ---

    #[test]
    fn clean_root_path() {
        assert_eq!(clean_route(&String::from("/")), "/");
    }

    #[test]
    fn clean_simple_path() {
        assert_eq!(clean_route(&String::from("/howdy")), "/howdy");
    }

    #[test]
    fn clean_nested_path() {
        assert_eq!(clean_route(&String::from("/a/b/c")), "/a/b/c");
    }

    #[test]
    fn clean_file_path() {
        assert_eq!(clean_route(&String::from("/index.css")), "/index.css");
    }

    #[test]
    fn clean_nested_file_path() {
        assert_eq!(clean_route(&String::from("/howdy/page.css")), "/howdy/page.css");
    }

    // --- Empty and minimal inputs ---

    #[test]
    fn clean_empty_string() {
        assert_eq!(clean_route(&String::from("")), "/");
    }

    #[test]
    fn clean_single_slash() {
        assert_eq!(clean_route(&String::from("/")), "/");
    }

    #[test]
    fn clean_double_slash() {
        assert_eq!(clean_route(&String::from("//")), "/");
    }

    #[test]
    fn clean_triple_slash() {
        assert_eq!(clean_route(&String::from("///")), "/");
    }

    #[test]
    fn clean_many_slashes() {
        assert_eq!(clean_route(&String::from("////////")), "/");
    }

    // --- Dot segments ---

    #[test]
    fn clean_single_dot() {
        assert_eq!(clean_route(&String::from("/.")), "/");
    }

    #[test]
    fn clean_single_dot_in_path() {
        assert_eq!(clean_route(&String::from("/howdy/./page.css")), "/howdy/page.css");
    }

    #[test]
    fn clean_multiple_dots() {
        assert_eq!(clean_route(&String::from("/./././.")), "/");
    }

    #[test]
    fn clean_dot_at_end() {
        assert_eq!(clean_route(&String::from("/howdy/.")), "/howdy");
    }

    // --- Path traversal (double dots) ---

    #[test]
    fn clean_dotdot_at_start() {
        assert_eq!(clean_route(&String::from("/..")), "/");
    }

    #[test]
    fn clean_dotdot_traversal() {
        assert_eq!(clean_route(&String::from("/../etc/passwd")), "/etc/passwd");
    }

    #[test]
    fn clean_deep_dotdot_traversal() {
        assert_eq!(
            clean_route(&String::from("/../../../../../../etc/passwd")),
            "/etc/passwd"
        );
    }

    #[test]
    fn clean_dotdot_in_middle() {
        assert_eq!(clean_route(&String::from("/howdy/../secret")), "/secret");
    }

    #[test]
    fn clean_dotdot_at_end() {
        assert_eq!(clean_route(&String::from("/howdy/..")), "/");
    }

    #[test]
    fn clean_multiple_dotdots() {
        assert_eq!(clean_route(&String::from("/a/../b/../c/../d")), "/d");
    }

    #[test]
    fn clean_dotdot_mixed_with_dots() {
        assert_eq!(clean_route(&String::from("/./../.././../etc")), "/etc");
    }

    // --- Trailing slashes ---

    #[test]
    fn clean_trailing_slash() {
        assert_eq!(clean_route(&String::from("/howdy/")), "/howdy");
    }

    #[test]
    fn clean_trailing_double_slash() {
        assert_eq!(clean_route(&String::from("/howdy//")), "/howdy");
    }

    // --- Double slashes in middle ---

    #[test]
    fn clean_double_slash_in_middle() {
        assert_eq!(clean_route(&String::from("/howdy//page.css")), "/howdy/page.css");
    }

    #[test]
    fn clean_multiple_slashes_in_middle() {
        assert_eq!(clean_route(&String::from("/a///b////c")), "/a/b/c");
    }

    // --- Adversarial patterns ---

    #[test]
    fn clean_dotdot_with_slashes() {
        assert_eq!(clean_route(&String::from("/..//..//..//etc/passwd")), "/etc/passwd");
    }

    #[test]
    fn clean_no_leading_slash() {
        // Route without leading slash
        assert_eq!(clean_route(&String::from("howdy")), "/howdy");
    }

    #[test]
    fn clean_just_dots_no_slashes() {
        // "." and ".." without slashes are treated as normal segments
        assert_eq!(clean_route(&String::from("...")), "/...");
    }

    #[test]
    fn clean_triple_dot_segment() {
        // "..." is NOT a special segment, should be kept
        assert_eq!(clean_route(&String::from("/howdy/...")), "/howdy/...");
    }

    #[test]
    fn clean_dot_prefix_filename() {
        // ".hidden" is a regular filename, not a dot segment
        assert_eq!(clean_route(&String::from("/.hidden")), "/.hidden");
    }

    #[test]
    fn clean_dotdot_prefix_filename() {
        // "..secret" is a regular filename, not a traversal
        assert_eq!(clean_route(&String::from("/..secret")), "/..secret");
    }

    #[test]
    fn clean_percent_encoded_dots_not_decoded() {
        // %2e is the percent-encoding of "." — clean_route does NOT decode
        // So "%2e%2e" is treated as a literal segment name, not ".."
        assert_eq!(clean_route(&String::from("/%2e%2e/etc/passwd")), "/%2e%2e/etc/passwd");
    }

    #[test]
    fn clean_spaces_in_path() {
        // Spaces should be preserved (they're not special to the splitter)
        assert_eq!(clean_route(&String::from("/path with spaces")), "/path with spaces");
    }

    #[test]
    fn clean_special_characters() {
        // Various special chars should be preserved
        assert_eq!(clean_route(&String::from("/a@b#c$d")), "/a@b#c$d");
    }

    #[test]
    fn clean_unicode_path() {
        assert_eq!(clean_route(&String::from("/café/naïve")), "/café/naïve");
    }

    #[test]
    fn clean_null_byte_in_path() {
        // Null bytes should be preserved (clean_route doesn't filter them)
        assert_eq!(clean_route(&String::from("/path\0with\0nulls")), "/path\0with\0nulls");
    }

    // --- Combinations ---

    #[test]
    fn clean_complex_adversarial() {
        assert_eq!(
            clean_route(&String::from("///..//./../../howdy/..///secret/./")),
            "/secret"
        );
    }

    #[test]
    fn clean_very_long_path() {
        let long_segment = "a".repeat(1000);
        let route = format!("/{long_segment}");
        assert_eq!(clean_route(&route), format!("/{long_segment}"));
    }

    #[test]
    fn clean_many_segments() {
        let route = "/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z";
        assert_eq!(clean_route(&String::from(route)), route);
    }
}
```

---

## Key Observations

### What `clean_route()` Does Well

1. Strips empty segments (double slashes)
2. Strips `.` segments
3. Strips `..` segments
4. Handles trailing slashes
5. Returns `/` for empty/root paths

### What `clean_route()` Does NOT Do

1. **Does not collapse `..` semantically**: `clean_route("/a/../b")` returns `/b`, not `/b`. Wait — actually, it strips `..` as a segment entirely, so `/a/../b` becomes `/a/b` (it keeps `a` and `b`, drops `..`). This is a **security concern** because:
   - Input: `/howdy/../secret` → Output: `/secret` (since `..` is just dropped, but `howdy` is kept)
   - Actually: splitting on `/` gives `["", "howdy", "..", "secret"]`. The function skips `""` and `".."`, keeps `"howdy"` and `"secret"`, producing `/howdy/secret`.
   - Wait, let me re-read: it skips parts equal to `""`, `"."`, or `".."`. So for `/howdy/../secret`, the parts are `["", "howdy", "..", "secret"]`. Skipping `""` and `".."` leaves `["howdy", "secret"]`, producing `/howdy/secret`.

**Important:** `clean_route()` does NOT perform proper path normalization. It simply removes `..` segments without actually traversing up a directory. This means:
- `/a/../b` → `/a/b` (NOT `/b`)
- `/../etc/passwd` → `/etc/passwd`

The result `/etc/passwd` is then looked up in the route hashmap. Since no route maps to `/etc/passwd`, it returns 404. The security comes from the hashmap, not from `clean_route()`.

### Tests Document This Behavior

The tests above document the actual behavior of `clean_route()`, including cases where the behavior differs from standard path normalization. This is intentional — the tests should match reality, not the ideal.

---

## Edge Cases & Considerations

### 1. `clean_route()` Is Not a Path Normalizer

The function removes `.` and `..` segments but doesn't resolve them relative to the path hierarchy. `/a/../b` becomes `/a/b`, not `/b`. This is safe because the route hashmap only contains valid routes.

### 2. Percent-Encoded Input

The function does not percent-decode. `%2e%2e` is treated as a literal segment, not `..`. This is currently safe but must be revisited when percent-decoding is implemented.

### 3. Null Bytes

The function does not filter null bytes. Null bytes in routes are preserved. This could be a concern for filesystem operations downstream.

### 4. Private Function

`clean_route()` is a private function in `main.rs`. Tests must be in the same file's `#[cfg(test)]` module, which has access to private items.

---

## Testing Strategy

### Running the Tests

```bash
cargo test clean_route
```

Or all tests:

```bash
cargo test
```

### Expected Results

All tests pass, documenting the current behavior of `clean_route()`.

---

## Implementation Checklist

- [ ] Add `#[cfg(test)] mod tests` to `src/main.rs`
- [ ] Add basic functionality tests (5 tests)
- [ ] Add empty/minimal input tests (5 tests)
- [ ] Add dot segment tests (4 tests)
- [ ] Add path traversal tests (7 tests)
- [ ] Add trailing slash tests (2 tests)
- [ ] Add double slash tests (2 tests)
- [ ] Add adversarial pattern tests (10+ tests)
- [ ] Add combination/stress tests (3 tests)
- [ ] Run `cargo test` — all tests pass

---

## Related Features

- **Security > Path Traversal Prevention**: Canonicalization-based defense-in-depth that complements `clean_route()`
- **HTTP Protocol > Percent-Decode URI**: Will require updating these tests when `clean_route()` handles decoded input
- **Security > Null Byte Rejection**: Separate feature to filter null bytes before they reach `clean_route()`

---

## References

- [RFC 3986 Section 5.2.4 - Remove Dot Segments](https://www.rfc-editor.org/rfc/rfc3986#section-5.2.4)
- [OWASP Path Traversal](https://owasp.org/www-community/attacks/Path_Traversal)
