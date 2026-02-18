# Route Aliases / Rewrite Rules Implementation Plan

## Overview

Route aliases allow operators to define URL rewrite rules that map one path to another without a client-visible redirect. For example, `/old-path` can transparently serve the content from `/new-path`, or `/api/v1/users` can be an alias for `/users.json`.

Unlike redirects (which return 301/302 and require the client to make a second request), aliases are server-side rewrites — the client sees the original URL but receives the content from the target route.

**Complexity**: 5
**Necessity**: 3

**Key Changes**:
- Define an alias configuration format (environment variable or config file)
- Parse alias rules at startup
- Apply alias resolution before route lookup in `handle_connection()`
- Support exact-match aliases and prefix-match aliases

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- `handle_connection()` (line 46) does route lookup with `routes.contains_key(&clean_target)` at line 62
- No alias/rewrite mechanism exists
- Routes are a flat `HashMap<String, PathBuf>`

**Changes Required**:
- Add alias configuration parsing
- Add alias resolution function
- Apply alias resolution in `handle_connection()` between `clean_route()` and route lookup
- Pass aliases to `handle_connection()`

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add integration tests for alias resolution

---

## Step-by-Step Implementation

### Step 1: Define Alias Configuration Format

Use the `RCOMM_ALIASES` environment variable with a semicolon-separated list of `source=target` pairs:

```bash
RCOMM_ALIASES="/old=/new;/blog=/articles;/home=/"
```

For prefix aliases (rewrite a path prefix), use a trailing `*`:

```bash
RCOMM_ALIASES="/api/*=/data/*;/v1/*=/v2/*"
```

### Step 2: Add Alias Types and Parser

**Location**: `src/main.rs`, before `main()`

```rust
/// A route alias mapping one path to another.
enum RouteAlias {
    /// Exact path match: /old -> /new
    Exact { source: String, target: String },
    /// Prefix match: /api/* -> /data/* (rewrites the prefix, keeps the rest)
    Prefix { source_prefix: String, target_prefix: String },
}

impl RouteAlias {
    /// If this alias matches the given path, return the rewritten path.
    fn apply(&self, path: &str) -> Option<String> {
        match self {
            RouteAlias::Exact { source, target } => {
                if path == source {
                    Some(target.clone())
                } else {
                    None
                }
            }
            RouteAlias::Prefix { source_prefix, target_prefix } => {
                if path.starts_with(source_prefix) {
                    let rest = &path[source_prefix.len()..];
                    Some(format!("{target_prefix}{rest}"))
                } else {
                    None
                }
            }
        }
    }
}

/// Parse alias configuration from RCOMM_ALIASES environment variable.
fn parse_aliases() -> Vec<RouteAlias> {
    let alias_str = match std::env::var("RCOMM_ALIASES") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    alias_str
        .split(';')
        .filter_map(|rule| {
            let rule = rule.trim();
            if rule.is_empty() {
                return None;
            }
            let parts: Vec<&str> = rule.splitn(2, '=').collect();
            if parts.len() != 2 {
                eprintln!("Warning: invalid alias rule: '{rule}'");
                return None;
            }
            let source = parts[0].trim();
            let target = parts[1].trim();

            if source.ends_with("/*") && target.ends_with("/*") {
                Some(RouteAlias::Prefix {
                    source_prefix: source.trim_end_matches('*').to_string(),
                    target_prefix: target.trim_end_matches('*').to_string(),
                })
            } else {
                Some(RouteAlias::Exact {
                    source: source.to_string(),
                    target: target.to_string(),
                })
            }
        })
        .collect()
}
```

### Step 3: Add Alias Resolution Function

```rust
/// Resolve a request path through aliases. Returns the rewritten path
/// or the original path if no alias matches.
/// Only applies the first matching alias (no chaining).
fn resolve_aliases(path: &str, aliases: &[RouteAlias]) -> String {
    for alias in aliases {
        if let Some(rewritten) = alias.apply(path) {
            return rewritten;
        }
    }
    path.to_string()
}
```

### Step 4: Integrate into `handle_connection()`

**Updated signature**:
```rust
fn handle_connection(
    mut stream: TcpStream,
    routes: HashMap<String, PathBuf>,
    aliases: Vec<RouteAlias>,
) {
```

**Add alias resolution** after `clean_route()` (after line 58):
```rust
    let clean_target = clean_route(&http_request.target);
    let resolved_target = resolve_aliases(&clean_target, &aliases);
```

**Update route lookup** (line 62):
```rust
    let (mut response, filename) = if routes.contains_key(&resolved_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&resolved_target).unwrap().to_str().unwrap())
    } else {
```

### Step 5: Update `main()`

```rust
    let aliases = parse_aliases();
    if !aliases.is_empty() {
        println!("Loaded {} route alias(es)", aliases.len());
    }

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let aliases_clone = aliases.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone, aliases_clone);
        });
    }
```

**Note**: `RouteAlias` needs to derive `Clone` for this to work:
```rust
#[derive(Clone)]
enum RouteAlias {
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod alias_tests {
    use super::*;

    #[test]
    fn exact_alias_matches() {
        let alias = RouteAlias::Exact {
            source: "/old".to_string(),
            target: "/new".to_string(),
        };
        assert_eq!(alias.apply("/old"), Some("/new".to_string()));
        assert_eq!(alias.apply("/other"), None);
    }

    #[test]
    fn prefix_alias_rewrites() {
        let alias = RouteAlias::Prefix {
            source_prefix: "/api/".to_string(),
            target_prefix: "/data/".to_string(),
        };
        assert_eq!(alias.apply("/api/users"), Some("/data/users".to_string()));
        assert_eq!(alias.apply("/api/"), Some("/data/".to_string()));
        assert_eq!(alias.apply("/other"), None);
    }

    #[test]
    fn resolve_aliases_returns_first_match() {
        let aliases = vec![
            RouteAlias::Exact { source: "/a".to_string(), target: "/b".to_string() },
            RouteAlias::Exact { source: "/a".to_string(), target: "/c".to_string() },
        ];
        assert_eq!(resolve_aliases("/a", &aliases), "/b");
    }

    #[test]
    fn resolve_aliases_returns_original_when_no_match() {
        let aliases = vec![
            RouteAlias::Exact { source: "/x".to_string(), target: "/y".to_string() },
        ];
        assert_eq!(resolve_aliases("/z", &aliases), "/z");
    }
}
```

### Integration Tests

Testing requires setting `RCOMM_ALIASES` on the server process. If the integration test spawns the server, it can set environment variables:

```rust
fn test_alias_serves_target_content(addr: &str) -> Result<(), String> {
    // Requires RCOMM_ALIASES="/alias=/" to be set on server
    let resp = send_request(addr, "GET", "/alias")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}
```

---

## Edge Cases & Handling

### 1. Alias Chain (A → B → C)
- **Behavior**: Only one level of resolution (no chaining)
- **Rationale**: Prevents infinite loops
- **Status**: By design

### 2. Alias Points to Non-existent Route
- **Behavior**: Route lookup fails, 404 returned
- **Status**: Correct behavior

### 3. Circular Alias (A → B, B → A)
- **Behavior**: Only first match applies; no loop
- **Status**: Safe

### 4. Alias Overlaps with Real Route
- **Behavior**: If `/old` is both an alias source and a real route, the alias takes priority (resolution happens before lookup)
- **Status**: Intentional — aliases override

### 5. Empty `RCOMM_ALIASES`
- **Behavior**: No aliases loaded, no behavioral change
- **Status**: Handled

---

## Implementation Checklist

- [ ] Define `RouteAlias` enum with `Exact` and `Prefix` variants
- [ ] Implement `RouteAlias::apply()` method
- [ ] Add `parse_aliases()` function
- [ ] Add `resolve_aliases()` function
- [ ] Update `handle_connection()` to resolve aliases before route lookup
- [ ] Update `main()` to parse and pass aliases
- [ ] Add unit tests for alias matching
- [ ] Add integration tests (requires env var setup)
- [ ] Run `cargo test` and `cargo run --bin integration_test`

---

## Backward Compatibility

When `RCOMM_ALIASES` is not set, no aliases are loaded and behavior is identical to current. All existing tests pass unchanged.
