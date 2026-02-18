# Fix Hardcoded Port Mismatch in HTML Pages

**Feature**: Fix the hardcoded port mismatch in HTML pages (`localhost:7879` vs. default port `7878`)
**Category**: Developer Experience
**Complexity**: 1/10
**Necessity**: 8/10

---

## Overview

The HTML files in `pages/` contain hardcoded absolute URLs pointing to `localhost:7879`, but the server's default port (via `RCOMM_PORT`) is `7878`. This means CSS stylesheets fail to load when the server runs on its default port, resulting in unstyled pages.

### Current State

All three HTML files reference `localhost:7879`:

**`pages/index.html` line 6**:
```html
<link rel="stylesheet" href="http://localhost:7879/index.css" />
```

**`pages/howdy/page.html` line 6**:
```html
<link rel="stylesheet" href="http://localhost:7879/howdy/page.css" />
```

**`pages/not_found.html` line 6**:
```html
<link rel="stylesheet" href="http://localhost:7879/index.css" />
```

**`src/main.rs` line 15** (default port):
```rust
fn get_port() -> String {
    std::env::var("RCOMM_PORT").unwrap_or_else(|_| String::from("7878"))
}
```

The mismatch: HTML says `7879`, server defaults to `7878`. CSS requests go to the wrong port and fail.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/pages/index.html`

**Line 6**: Change port from `7879` to `7878`

### 2. `/home/jwall/personal/rusty/rcomm/pages/howdy/page.html`

**Line 6**: Change port from `7879` to `7878`

### 3. `/home/jwall/personal/rusty/rcomm/pages/not_found.html`

**Line 6**: Change port from `7879` to `7878`

---

## Step-by-Step Implementation

### Step 1: Fix `pages/index.html`

**Current** (line 6):
```html
<link rel="stylesheet" href="http://localhost:7879/index.css" />
```

**Updated**:
```html
<link rel="stylesheet" href="http://localhost:7878/index.css" />
```

### Step 2: Fix `pages/howdy/page.html`

**Current** (line 6):
```html
<link rel="stylesheet" href="http://localhost:7879/howdy/page.css" />
```

**Updated**:
```html
<link rel="stylesheet" href="http://localhost:7878/howdy/page.css" />
```

### Step 3: Fix `pages/not_found.html`

**Current** (line 6):
```html
<link rel="stylesheet" href="http://localhost:7879/index.css" />
```

**Updated**:
```html
<link rel="stylesheet" href="http://localhost:7878/index.css" />
```

---

## Edge Cases & Handling

### 1. Custom Port via `RCOMM_PORT`
**Scenario**: User runs the server with `RCOMM_PORT=3000`.
**Handling**: The hardcoded URLs will still point to `localhost:7878`, so CSS will break on non-default ports. This is a known limitation of hardcoded absolute URLs — the "Support relative CSS/JS paths" feature (separate) addresses this properly. This fix only corrects the mismatch with the default port.

### 2. Integration Tests
**Scenario**: Integration tests run the server on a random port.
**Handling**: Integration tests verify HTTP responses, not rendered HTML. The CSS URL in the HTML body doesn't affect test outcomes. The port mismatch in HTML is only visible when a browser renders the page.

### 3. Custom Address via `RCOMM_ADDRESS`
**Scenario**: User binds to `0.0.0.0` or a specific IP.
**Handling**: The HTML references `localhost`, not the bind address. This is another limitation of hardcoded absolute URLs, also addressed by the relative paths feature.

---

## Implementation Checklist

- [ ] Update `pages/index.html` line 6: `7879` → `7878`
- [ ] Update `pages/howdy/page.html` line 6: `7879` → `7878`
- [ ] Update `pages/not_found.html` line 6: `7879` → `7878`
- [ ] Run `cargo build` to verify compilation (no Rust changes)
- [ ] Run `cargo test` to verify unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify integration tests pass
- [ ] Manual test: start server on default port, verify CSS loads in browser

---

## Backward Compatibility

This fixes a bug — CSS has been broken on the default port. The change makes the default configuration work correctly. Users who were running on port `7879` (matching the old hardcoded value) will now see CSS break for them instead, but this is the correct trade-off: the default should work.

---

## Related Features

- **Developer Experience > Support Relative CSS/JS Paths**: The proper fix is to use relative paths (e.g., `/index.css`) instead of absolute URLs. That feature eliminates the port dependency entirely and makes this fix redundant. However, this fix is trivial and should be applied immediately since relative paths require more thought about the routing system.
