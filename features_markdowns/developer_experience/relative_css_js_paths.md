# Support Relative CSS/JS Paths in HTML Pages

**Feature**: Support relative CSS/JS paths in HTML pages instead of hardcoded absolute URLs
**Category**: Developer Experience
**Complexity**: 1/10
**Necessity**: 7/10

---

## Overview

The HTML files in `pages/` use fully-qualified absolute URLs for CSS references (e.g., `http://localhost:7879/index.css`). This means the pages only work when served from `localhost` on that exact port. Switching to root-relative paths (e.g., `/index.css`) makes the pages work on any host, any port, and any protocol — the browser resolves the path relative to the current server.

### Current State

All three HTML files use absolute URLs with hardcoded host and port:

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

### Desired State

```html
<link rel="stylesheet" href="/index.css" />
```

```html
<link rel="stylesheet" href="/howdy/page.css" />
```

```html
<link rel="stylesheet" href="/index.css" />
```

Root-relative paths (starting with `/`) tell the browser: "request this path from the same server that served this page." The browser automatically uses the current protocol, host, and port.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/pages/index.html`

**Line 6**: Replace absolute URL with root-relative path

### 2. `/home/jwall/personal/rusty/rcomm/pages/howdy/page.html`

**Line 6**: Replace absolute URL with root-relative path

### 3. `/home/jwall/personal/rusty/rcomm/pages/not_found.html`

**Line 6**: Replace absolute URL with root-relative path

---

## Step-by-Step Implementation

### Step 1: Fix `pages/index.html`

**Current** (line 6):
```html
<link rel="stylesheet" href="http://localhost:7879/index.css" />
```

**Updated**:
```html
<link rel="stylesheet" href="/index.css" />
```

### Step 2: Fix `pages/howdy/page.html`

**Current** (line 6):
```html
<link rel="stylesheet" href="http://localhost:7879/howdy/page.css" />
```

**Updated**:
```html
<link rel="stylesheet" href="/howdy/page.css" />
```

### Step 3: Fix `pages/not_found.html`

**Current** (line 6):
```html
<link rel="stylesheet" href="http://localhost:7879/index.css" />
```

**Updated**:
```html
<link rel="stylesheet" href="/index.css" />
```

---

## Why Root-Relative, Not Document-Relative?

There are three types of paths in HTML:

| Type | Example | Resolves to |
|------|---------|-------------|
| Absolute | `http://localhost:7879/index.css` | Exactly that URL |
| Root-relative | `/index.css` | `http://<current-server>/index.css` |
| Document-relative | `index.css` or `../index.css` | Relative to the current page's path |

**Root-relative** (`/index.css`) is the right choice because:
- Works regardless of host, port, or protocol
- Works regardless of what URL path the current page is at
- Matches the routes that the server already defines (e.g., `/index.css` is a registered route)
- No ambiguity: `/howdy/page.css` always means the same thing whether the browser is at `/`, `/howdy`, or `/other`

**Document-relative** (`index.css`) would break for nested pages. If `pages/howdy/page.html` referenced `page.css` (document-relative), the browser at `/howdy` would request `/howdy/page.css`, which works. But if the trailing-slash feature is added and the browser is at `/howdy/`, a document-relative `page.css` would resolve to `/howdy/page.css` — which also works in this case. However, root-relative avoids all such ambiguity.

---

## Edge Cases & Handling

### 1. Reverse Proxy with Path Prefix
**Scenario**: Server is behind a reverse proxy at `example.com/app/`, so the root is not `/` but `/app/`.
**Handling**: Root-relative paths would break in this scenario (`/index.css` would resolve to `example.com/index.css`, not `example.com/app/index.css`). This is a limitation, but it's the same limitation every static server has. The solution would be a configurable base path, which is out of scope.

### 2. Future JavaScript Files
**Scenario**: `<script>` tags are added to HTML pages.
**Handling**: Apply the same pattern: use root-relative paths (`/script.js`) instead of absolute URLs.

### 3. Images and Other Assets
**Scenario**: HTML pages reference images via `<img src="...">`.
**Handling**: Same pattern. All asset references should use root-relative paths.

---

## Implementation Checklist

- [ ] Update `pages/index.html` line 6: `href="http://localhost:7879/index.css"` → `href="/index.css"`
- [ ] Update `pages/howdy/page.html` line 6: `href="http://localhost:7879/howdy/page.css"` → `href="/howdy/page.css"`
- [ ] Update `pages/not_found.html` line 6: `href="http://localhost:7879/index.css"` → `href="/index.css"`
- [ ] Run `cargo build` to verify compilation (no Rust changes)
- [ ] Run `cargo test` to verify unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify integration tests pass
- [ ] Manual test: start server, verify CSS loads in browser on default port
- [ ] Manual test: start server with `RCOMM_PORT=3000`, verify CSS still loads

---

## Backward Compatibility

This is a purely beneficial change. The hardcoded absolute URLs were always fragile — they only worked when the server ran on `localhost:7879`. After this change, the pages work on any host and port. There is no scenario where the old behavior was preferable.

Note: This feature supersedes the "Fix hardcoded port mismatch" feature. If relative paths are implemented, the port mismatch fix becomes unnecessary. However, if the port fix is applied first (it's trivial), it still works correctly — relative paths simply remove the port dependency entirely.

---

## Related Features

- **Developer Experience > Fix Hardcoded Port Mismatch**: That feature changes `7879` to `7878`; this feature removes the host:port entirely. This is the more complete fix.
- **HTTP Protocol Compliance > Content-Type Header**: The server needs to set `Content-Type: text/css` for `.css` files so the browser uses them as stylesheets. Without the correct `Content-Type`, some browsers may refuse to apply the CSS even if the file loads correctly.
