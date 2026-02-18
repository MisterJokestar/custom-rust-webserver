# Dotfile Serving Configuration Implementation Plan

## Overview

Dotfiles are files and directories whose names begin with a period (e.g., `.env`, `.htaccess`, `.git/`). By convention on Unix systems, these are hidden and often contain sensitive configuration data. Currently, rcomm's `build_routes()` function does not explicitly filter dotfiles — it would route them if their extension matched `"html" | "css" | "js"`. However, it also does not explicitly handle dotfile directories during recursive scanning.

This feature adds explicit dotfile handling:
1. **Default behavior**: Skip all dotfiles and dot-directories during route building (security-first)
2. **Opt-in configuration**: Allow serving dotfiles via an environment variable `RCOMM_SERVE_DOTFILES=true`
3. **Always block**: Never serve `.env`, `.git/`, `.gitignore`, or other security-sensitive dotfiles regardless of configuration

**Complexity**: 2
**Necessity**: 3

**Key Changes**:
- Add dotfile detection in `build_routes()`
- Add environment variable `RCOMM_SERVE_DOTFILES` for opt-in
- Add a hardcoded blocklist of never-serve dotfiles
- Filter dotfile requests in `handle_connection()` as a safety net

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- `build_routes()` (line 91) recursively scans `pages/` directory
- At line 98, it recurses into subdirectories without checking if they are dot-directories
- At line 103, it checks file extensions but not whether the filename starts with `.`
- No environment variable for dotfile configuration

**Changes Required**:
- Add dotfile detection at both the directory recursion level (line 98) and file routing level (line 102)
- Read `RCOMM_SERVE_DOTFILES` environment variable
- Add blocked dotfile list
- Add safety check in `handle_connection()` as defense-in-depth

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add integration tests verifying dotfiles return 404 by default
- Add integration tests verifying dotfiles are served when `RCOMM_SERVE_DOTFILES=true`
- Add integration tests verifying blocklisted dotfiles always return 404

---

## Step-by-Step Implementation

### Step 1: Add Dotfile Configuration Reader

**Location**: `src/main.rs`, after `get_address()` (after line 20)

```rust
fn get_serve_dotfiles() -> bool {
    std::env::var("RCOMM_SERVE_DOTFILES")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

/// Dotfiles that are NEVER served regardless of configuration.
/// These commonly contain secrets or internal metadata.
const BLOCKED_DOTFILES: &[&str] = &[
    ".env",
    ".git",
    ".gitignore",
    ".gitmodules",
    ".hg",
    ".svn",
    ".DS_Store",
    ".htaccess",
    ".htpasswd",
    ".npmrc",
    ".dockerignore",
];
```

### Step 2: Add Dotfile Detection Helper

**Location**: `src/main.rs`, after the blocked dotfile list

```rust
/// Returns true if a filename starts with a dot (is a dotfile/dotdirectory).
fn is_dotfile(name: &str) -> bool {
    name.starts_with('.')
}

/// Returns true if a dotfile is in the always-blocked list.
fn is_blocked_dotfile(name: &str) -> bool {
    BLOCKED_DOTFILES.iter().any(|&blocked| name == blocked)
}
```

### Step 3: Modify `build_routes()` to Filter Dotfiles

**Location**: `src/main.rs`, lines 91-123

**Current Code** (lines 94-101):
```rust
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
```

**New Code**:
```rust
fn build_routes(route: String, directory: &Path, serve_dotfiles: bool) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();

        // Skip dotfiles/dotdirs unless opt-in, always skip blocked ones
        if is_dotfile(name) {
            if is_blocked_dotfile(name) || !serve_dotfiles {
                continue;
            }
        }

        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path, serve_dotfiles)
            );
```

**Update call site in `main()`** (line 31):
```rust
    let serve_dotfiles = get_serve_dotfiles();
    let routes = build_routes(String::from(""), path, serve_dotfiles);
```

### Step 4: Add Safety Check in `handle_connection()`

**Location**: `src/main.rs`, inside `handle_connection()`, after `clean_target` (line 58)

This is a defense-in-depth measure — even if a dotfile somehow gets into the routes map, block it at request time:

```rust
    // Defense-in-depth: block dotfile requests that shouldn't be served
    let target_segments: Vec<&str> = clean_target.split('/').collect();
    let has_blocked_dotfile = target_segments.iter().any(|segment| {
        is_dotfile(segment) && is_blocked_dotfile(segment)
    });
    if has_blocked_dotfile {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 404);
        let contents = fs::read_to_string("pages/not_found.html").unwrap();
        response.add_body(contents.into());
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    }
```

---

### Step 5: Add Integration Tests

**Location**: `src/bin/integration_test.rs`

#### 5a. Create Test Dotfile

Create `pages/.testdotfile.html` for testing:
```html
<p>dotfile test</p>
```

#### 5b. Add Test Functions

```rust
fn test_dotfile_blocked_by_default(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/.testdotfile.html")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    Ok(())
}

fn test_env_file_always_blocked(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/.env")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    Ok(())
}

fn test_git_directory_always_blocked(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/.git/config")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    Ok(())
}
```

#### 5c. Register Tests

```rust
    run_test("dotfile_blocked_by_default", || test_dotfile_blocked_by_default(&addr)),
    run_test("env_file_always_blocked", || test_env_file_always_blocked(&addr)),
    run_test("git_directory_always_blocked", || test_git_directory_always_blocked(&addr)),
```

---

## Testing Strategy

### Unit Tests

Add tests in `src/main.rs`:

```rust
#[cfg(test)]
mod dotfile_tests {
    use super::*;

    #[test]
    fn is_dotfile_detects_dot_prefix() {
        assert!(is_dotfile(".env"));
        assert!(is_dotfile(".git"));
        assert!(is_dotfile(".htaccess"));
        assert!(!is_dotfile("index.html"));
        assert!(!is_dotfile("style.css"));
    }

    #[test]
    fn is_blocked_dotfile_matches_blocklist() {
        assert!(is_blocked_dotfile(".env"));
        assert!(is_blocked_dotfile(".git"));
        assert!(is_blocked_dotfile(".htaccess"));
        assert!(!is_blocked_dotfile(".custom"));
        assert!(!is_blocked_dotfile(".myconfig"));
    }

    #[test]
    fn empty_name_is_not_dotfile() {
        assert!(!is_dotfile(""));
    }
}
```

### Integration Tests

| Test Name | What It Validates |
|-----------|-------------------|
| `dotfile_blocked_by_default` | Dotfiles return 404 when `RCOMM_SERVE_DOTFILES` is not set |
| `env_file_always_blocked` | `.env` always returns 404 regardless of configuration |
| `git_directory_always_blocked` | `.git/*` paths always return 404 |

### Manual Testing

```bash
# Create test dotfile
echo "<p>test</p>" > pages/.test.html

# Default: dotfiles blocked
cargo run &
curl -i http://127.0.0.1:7878/.test.html
# Expected: 404

# Opt-in: dotfiles served
kill %1
RCOMM_SERVE_DOTFILES=true cargo run &
curl -i http://127.0.0.1:7878/.test.html
# Expected: 200

# Blocked dotfiles always blocked
curl -i http://127.0.0.1:7878/.env
# Expected: 404

# Cleanup
rm pages/.test.html
```

---

## Edge Cases & Handling

### 1. Dotfile in Subdirectory
- **Example**: `pages/howdy/.secret.html`
- **Behavior**: Skipped during `build_routes()` recursion
- **Status**: Handled by checking `is_dotfile(name)` for every entry

### 2. Dot-directory Contains Normal Files
- **Example**: `pages/.hidden/index.html`
- **Behavior**: Entire `.hidden/` directory skipped (not recursed into)
- **Status**: Handled by the directory check in `build_routes()`

### 3. `.env` File in Any Subdirectory
- **Example**: `pages/config/.env`
- **Behavior**: Always blocked (checked at both build and request time)
- **Status**: Defense-in-depth

### 4. Double Dots (`..`)
- **Behavior**: Already handled by `clean_route()` which strips `..` segments (line 80)
- **Status**: Not a dotfile issue; path traversal prevention

### 5. File Named `.` or `..`
- **Behavior**: Filtered by `clean_route()` and also by `is_dotfile()` check
- **Status**: Double-protected

### 6. Dotfile Without Extension
- **Example**: `.env` (no `.html`/`.css`/`.js` extension)
- **Behavior**: Even without dotfile filtering, would be skipped by the extension match (line 103)
- **Status**: Protected by existing logic + new dotfile logic = defense-in-depth

### 7. `RCOMM_SERVE_DOTFILES` Set to Unexpected Value
- **Example**: `RCOMM_SERVE_DOTFILES=yes` or `RCOMM_SERVE_DOTFILES=TRUE`
- **Behavior**: Only `"true"` and `"1"` are accepted; anything else defaults to `false`
- **Status**: Conservative default

---

## Implementation Checklist

- [ ] Add `get_serve_dotfiles()` function to `src/main.rs`
- [ ] Add `BLOCKED_DOTFILES` constant to `src/main.rs`
- [ ] Add `is_dotfile()` helper function
- [ ] Add `is_blocked_dotfile()` helper function
- [ ] Modify `build_routes()` signature to accept `serve_dotfiles: bool`
- [ ] Add dotfile filtering in `build_routes()` loop
- [ ] Update `build_routes()` call in `main()` to pass `serve_dotfiles`
- [ ] Add defense-in-depth dotfile check in `handle_connection()`
- [ ] Add unit tests for `is_dotfile()` and `is_blocked_dotfile()`
- [ ] Create test dotfile in `pages/` for integration testing
- [ ] Add integration tests for default blocking
- [ ] Add integration tests for always-blocked files
- [ ] Run `cargo test` to verify unit tests
- [ ] Run `cargo run --bin integration_test` to verify integration tests
- [ ] Clean up test dotfiles

---

## Backward Compatibility

### Existing Tests
All existing tests pass without modification. The only behavioral change is that dotfiles (which were previously served if they matched the extension filter) are now blocked by default.

### Behavioral Changes
- **Breaking (intentional)**: Dotfiles that were previously routable (e.g., `.page.html` in `pages/`) will now return 404 by default. This is a security improvement.
- **Opt-in restore**: Set `RCOMM_SERVE_DOTFILES=true` to restore previous behavior (except for blocked dotfiles).

### API Changes
- `build_routes()` gains a new `serve_dotfiles: bool` parameter
- New environment variable: `RCOMM_SERVE_DOTFILES`

### Performance Impact
- **Negligible**: One `starts_with('.')` check per directory entry during route building
- **Request-time check**: One string comparison per path segment for blocked dotfiles
