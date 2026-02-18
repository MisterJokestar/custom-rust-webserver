# File Extension Whitelist/Blacklist Implementation Plan

## Overview

Currently, rcomm hardcodes the set of allowed file extensions in `build_routes()` at line 103-104 of `src/main.rs`:

```rust
match path.extension().unwrap().to_str().unwrap() {
    "html" | "css" | "js" => {
```

Adding support for new file types requires modifying source code and recompiling. This feature introduces configurable file extension filtering via environment variables, allowing operators to control which file types are served without code changes.

Two modes are supported:
1. **Whitelist mode** (`RCOMM_EXTENSIONS_ALLOW`): Only serve files with listed extensions (replaces the hardcoded list)
2. **Blacklist mode** (`RCOMM_EXTENSIONS_DENY`): Serve all files EXCEPT those with listed extensions

If neither is set, the current hardcoded behavior (`html`, `css`, `js`) is preserved as the default whitelist.

**Complexity**: 3
**Necessity**: 4

**Key Changes**:
- Parse `RCOMM_EXTENSIONS_ALLOW` and `RCOMM_EXTENSIONS_DENY` environment variables
- Replace the hardcoded extension match in `build_routes()` with a configurable filter
- Maintain the `index.html`/`page.html`/`not_found.html` convention-based routing logic
- Add validation to prevent setting both whitelist and blacklist simultaneously

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- `build_routes()` at line 91 scans `pages/` recursively
- Line 103-104: Hardcoded `"html" | "css" | "js"` extension match
- Lines 105-115: Convention-based routing for `index.html`, `page.html`, `not_found.html`
- Line 117: Unknown extensions are skipped with `continue`

**Changes Required**:
- Add extension filter configuration struct/enum
- Parse environment variables at startup
- Pass the filter configuration to `build_routes()`
- Replace hardcoded extension match with configurable filter
- Preserve convention-based routing for `index.html`/`page.html`/`not_found.html` regardless of filter

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add integration tests for custom extension whitelist
- Add integration tests for extension blacklist
- Add integration tests verifying default behavior is unchanged

---

## Step-by-Step Implementation

### Step 1: Define Extension Filter Types

**Location**: `src/main.rs`, before `build_routes()` (before line 91)

```rust
/// Configuration for which file extensions are allowed to be served.
enum ExtensionFilter {
    /// Only serve files with these extensions (whitelist mode).
    Allow(Vec<String>),
    /// Serve all files EXCEPT those with these extensions (blacklist mode).
    Deny(Vec<String>),
}

impl ExtensionFilter {
    /// Returns true if the given extension should be served.
    fn is_allowed(&self, extension: &str) -> bool {
        let ext = extension.to_lowercase();
        match self {
            ExtensionFilter::Allow(allowed) => {
                allowed.iter().any(|a| a == &ext)
            }
            ExtensionFilter::Deny(denied) => {
                !denied.iter().any(|d| d == &ext)
            }
        }
    }
}
```

### Step 2: Add Configuration Parser

**Location**: `src/main.rs`, after `get_address()` (after line 20)

```rust
/// Parse extension filter configuration from environment variables.
///
/// - RCOMM_EXTENSIONS_ALLOW: Comma-separated whitelist (e.g., "html,css,js,png,svg")
/// - RCOMM_EXTENSIONS_DENY: Comma-separated blacklist (e.g., "exe,dll,so")
/// - If neither is set, defaults to Allow("html,css,js")
/// - Setting both is an error (prints warning, falls back to default)
fn get_extension_filter() -> ExtensionFilter {
    let allow_var = std::env::var("RCOMM_EXTENSIONS_ALLOW").ok();
    let deny_var = std::env::var("RCOMM_EXTENSIONS_DENY").ok();

    match (allow_var, deny_var) {
        (Some(_), Some(_)) => {
            eprintln!(
                "Warning: Both RCOMM_EXTENSIONS_ALLOW and RCOMM_EXTENSIONS_DENY are set. \
                 Using RCOMM_EXTENSIONS_ALLOW only."
            );
            let allow = std::env::var("RCOMM_EXTENSIONS_ALLOW").unwrap();
            ExtensionFilter::Allow(parse_extension_list(&allow))
        }
        (Some(allow), None) => {
            ExtensionFilter::Allow(parse_extension_list(&allow))
        }
        (None, Some(deny)) => {
            ExtensionFilter::Deny(parse_extension_list(&deny))
        }
        (None, None) => {
            // Default: same as current hardcoded behavior
            ExtensionFilter::Allow(vec![
                "html".to_string(),
                "css".to_string(),
                "js".to_string(),
            ])
        }
    }
}

/// Parse a comma-separated list of extensions, trimming whitespace and dots.
/// "html, .css, JS" -> ["html", "css", "js"]
fn parse_extension_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().trim_start_matches('.').to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}
```

### Step 3: Modify `build_routes()` to Accept Extension Filter

**Location**: `src/main.rs`, line 91

**Current Signature**:
```rust
fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
```

**New Signature**:
```rust
fn build_routes(
    route: String,
    directory: &Path,
    ext_filter: &ExtensionFilter,
) -> HashMap<String, PathBuf> {
```

**Modify the extension matching** (lines 102-118):

**Current Code**:
```rust
        } else if path.is_file() {
            match path.extension().unwrap().to_str().unwrap() {
                "html" | "css" | "js" => {
                    if name == "index.html" || name == "page.html" {
                        if route == "" {
                            routes.insert(String::from("/"), path);
                        } else {
                            routes.insert(route.clone(), path);
                        }
                    } else if name == "not_found.html" {
                        continue;
                    } else {
                        routes.insert(format!("{route}/{name}"), path);
                    }
                }
                _ => {continue;}
            }
```

**New Code**:
```rust
        } else if path.is_file() {
            let extension = match path.extension().and_then(|e| e.to_str()) {
                Some(ext) => ext,
                None => continue, // Skip extensionless files
            };

            // Always skip not_found.html (used as 404 template, not a route)
            if name == "not_found.html" {
                continue;
            }

            // Check if extension is allowed by the filter
            if !ext_filter.is_allowed(extension) {
                continue;
            }

            // Convention-based routing for index.html and page.html
            if name == "index.html" || name == "page.html" {
                if route == "" {
                    routes.insert(String::from("/"), path);
                } else {
                    routes.insert(route.clone(), path);
                }
            } else {
                routes.insert(format!("{route}/{name}"), path);
            }
```

**Update recursive call** (line 99-101):
```rust
            routes.extend(
                build_routes(format!("{route}/{name}"), &path, ext_filter)
            );
```

### Step 4: Update `main()` to Pass Extension Filter

**Location**: `src/main.rs`, lines 30-31

**Current Code**:
```rust
    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);
```

**New Code**:
```rust
    let path = Path::new("./pages");
    let ext_filter = get_extension_filter();
    let routes = build_routes(String::from(""), path, &ext_filter);
```

### Step 5: Print Configuration on Startup

**Location**: `src/main.rs`, after route building (around line 33)

```rust
    match &ext_filter {
        ExtensionFilter::Allow(exts) => {
            println!("Extension whitelist: {}", exts.join(", "));
        }
        ExtensionFilter::Deny(exts) => {
            println!("Extension blacklist: {}", exts.join(", "));
        }
    }
```

---

### Step 6: Add Integration Tests

**Location**: `src/bin/integration_test.rs`

**Note**: Since the integration test binary spawns the server as a child process, environment variables can be set on the child process to test different configurations.

```rust
fn test_default_extensions_serve_html(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}

fn test_default_extensions_serve_css(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}
```

For testing non-default configurations, the integration test binary would need to be extended to spawn the server with different environment variables. This is a more advanced test scenario:

```rust
// Example: test with custom RCOMM_EXTENSIONS_ALLOW
// Would require spawning a separate server instance with:
//   RCOMM_EXTENSIONS_ALLOW=html
// Then verifying that CSS files return 404
```

---

## Testing Strategy

### Unit Tests

Add tests in `src/main.rs`:

```rust
#[cfg(test)]
mod extension_filter_tests {
    use super::*;

    #[test]
    fn allow_filter_accepts_listed_extensions() {
        let filter = ExtensionFilter::Allow(vec![
            "html".to_string(),
            "css".to_string(),
        ]);
        assert!(filter.is_allowed("html"));
        assert!(filter.is_allowed("css"));
        assert!(!filter.is_allowed("js"));
        assert!(!filter.is_allowed("png"));
    }

    #[test]
    fn deny_filter_rejects_listed_extensions() {
        let filter = ExtensionFilter::Deny(vec![
            "exe".to_string(),
            "dll".to_string(),
        ]);
        assert!(filter.is_allowed("html"));
        assert!(filter.is_allowed("css"));
        assert!(!filter.is_allowed("exe"));
        assert!(!filter.is_allowed("dll"));
    }

    #[test]
    fn allow_filter_case_insensitive() {
        let filter = ExtensionFilter::Allow(vec!["html".to_string()]);
        assert!(filter.is_allowed("HTML"));
        assert!(filter.is_allowed("Html"));
        assert!(filter.is_allowed("html"));
    }

    #[test]
    fn parse_extension_list_handles_whitespace() {
        let result = parse_extension_list("html, css , js");
        assert_eq!(result, vec!["html", "css", "js"]);
    }

    #[test]
    fn parse_extension_list_strips_dots() {
        let result = parse_extension_list(".html,.css,.js");
        assert_eq!(result, vec!["html", "css", "js"]);
    }

    #[test]
    fn parse_extension_list_lowercases() {
        let result = parse_extension_list("HTML,CSS,JS");
        assert_eq!(result, vec!["html", "css", "js"]);
    }

    #[test]
    fn parse_extension_list_skips_empty() {
        let result = parse_extension_list("html,,css,  ,js");
        assert_eq!(result, vec!["html", "css", "js"]);
    }
}
```

### Integration Tests

| Test Name | What It Validates |
|-----------|-------------------|
| `default_extensions_serve_html` | HTML files served with default config |
| `default_extensions_serve_css` | CSS files served with default config |

### Manual Testing

```bash
# Default behavior (html, css, js)
cargo run &
curl -i http://127.0.0.1:7878/        # 200 (html)
curl -i http://127.0.0.1:7878/index.css  # 200 (css)
kill %1

# Custom whitelist: only HTML
RCOMM_EXTENSIONS_ALLOW=html cargo run &
curl -i http://127.0.0.1:7878/        # 200 (html)
curl -i http://127.0.0.1:7878/index.css  # 404 (css not in whitelist)
kill %1

# Extended whitelist: html, css, js, png, svg
RCOMM_EXTENSIONS_ALLOW="html,css,js,png,svg" cargo run &
curl -i http://127.0.0.1:7878/        # 200
kill %1

# Blacklist mode: deny only exe
RCOMM_EXTENSIONS_DENY=exe cargo run &
curl -i http://127.0.0.1:7878/        # 200 (html allowed)
curl -i http://127.0.0.1:7878/index.css  # 200 (css allowed)
kill %1
```

---

## Edge Cases & Handling

### 1. Both `RCOMM_EXTENSIONS_ALLOW` and `RCOMM_EXTENSIONS_DENY` Set
- **Behavior**: Print warning to stderr, use `RCOMM_EXTENSIONS_ALLOW` only
- **Rationale**: Whitelist is more restrictive (safer default)
- **Status**: Handled in `get_extension_filter()`

### 2. Empty Environment Variable
- **Example**: `RCOMM_EXTENSIONS_ALLOW=""`
- **Behavior**: `parse_extension_list("")` returns empty vec; `is_allowed()` returns false for everything
- **Result**: No files are served (effectively disables file serving)
- **Status**: Acceptable edge case; operator chose this configuration

### 3. Extension with Leading Dot
- **Example**: `RCOMM_EXTENSIONS_ALLOW=".html,.css"`
- **Behavior**: `trim_start_matches('.')` strips the dot
- **Status**: Handled by `parse_extension_list()`

### 4. Mixed Case Extensions
- **Example**: `RCOMM_EXTENSIONS_ALLOW="HTML,Css,js"`
- **Behavior**: All lowercased during parsing; matching is case-insensitive
- **Status**: Handled by `.to_lowercase()` in both parser and filter

### 5. Files Without Extensions
- **Behavior**: Skipped with `continue` (no extension to check against filter)
- **Status**: Consistent with current behavior (`.unwrap()` on extension would panic; now safely handled)

### 6. `not_found.html` Convention
- **Behavior**: Always skipped regardless of extension filter (checked before filter)
- **Status**: Convention preserved

### 7. `index.html`/`page.html` Convention
- **Behavior**: Convention-based routing still applies when `html` is in the whitelist
- **Status**: If `html` is not in the whitelist, `index.html`/`page.html` are not routed (correct behavior — operator explicitly excluded HTML)

### 8. Blacklist Mode Serves All Other Extensions
- **Behavior**: Any file in `pages/` with an extension not in the deny list is served
- **Note**: This includes potentially dangerous files if placed in `pages/`
- **Mitigation**: The `pages/` directory is controlled by the operator; document this behavior

---

## Implementation Checklist

- [ ] Add `ExtensionFilter` enum to `src/main.rs`
- [ ] Implement `ExtensionFilter::is_allowed()` method
- [ ] Add `get_extension_filter()` function
- [ ] Add `parse_extension_list()` function
- [ ] Modify `build_routes()` signature to accept `&ExtensionFilter`
- [ ] Replace hardcoded extension match with `ext_filter.is_allowed()` call
- [ ] Handle extensionless files gracefully (no `.unwrap()` on extension)
- [ ] Preserve `not_found.html` skip and `index.html`/`page.html` convention
- [ ] Update recursive `build_routes()` call to pass filter
- [ ] Update `main()` to create and pass `ExtensionFilter`
- [ ] Add startup log message showing active filter
- [ ] Add unit tests for `ExtensionFilter::is_allowed()`
- [ ] Add unit tests for `parse_extension_list()`
- [ ] Add integration tests for default behavior
- [ ] Run `cargo test` to verify all unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify all integration tests pass
- [ ] Manual test with `RCOMM_EXTENSIONS_ALLOW` and `RCOMM_EXTENSIONS_DENY`

---

## Backward Compatibility

### Existing Tests
All existing tests pass without modification. When no environment variables are set, the default `ExtensionFilter::Allow(["html", "css", "js"])` exactly replicates the current hardcoded behavior.

### Behavioral Changes
- **No change by default**: Without environment variables, behavior is identical
- **New capability**: Operators can now expand or restrict served file types via environment variables

### API Changes
- `build_routes()` gains a new `ext_filter: &ExtensionFilter` parameter (internal API)
- New environment variables: `RCOMM_EXTENSIONS_ALLOW`, `RCOMM_EXTENSIONS_DENY`

### Bug Fix (Bonus)
- The current code uses `.unwrap()` on `path.extension()` (line 103), which panics for extensionless files
- The new implementation uses `.and_then()` with a `continue` fallback, fixing this latent bug

### Performance Impact
- **Negligible**: One `Vec::iter().any()` call per file during route building
- **Route building is once at startup**: No per-request overhead
