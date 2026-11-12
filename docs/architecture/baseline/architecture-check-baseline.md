# Architecture check report

**Mode: REPORT-ONLY.** Exit status is 0 regardless of findings.

## Workspace `Cargo.toml`

- Members: 1
- Internal edges: 0
- Path dependencies outside this workspace: 3
- Violations: 0
- Informational: 1

### Path dependencies outside this workspace

- wega -> wega-network (normal)
- wega -> wega-platform (normal)
- wega -> wega-protocol (normal)

### Violations

None.

### Informational

- `ARCH-007 third-party engines only in adapters (future target)`: wega depends directly on `servo` (normal)

## Workspace `engines/Cargo.toml`

- Members: 4
- Internal edges: 6
- Path dependencies outside this workspace: 0
- Violations: 0
- Informational: 14

### Violations

None.

### Informational

- `ARCH-006 forbid(unsafe_code) by default`: wega-engine does not declare #![forbid(unsafe_code)] (legacy crate; documented exception/not yet reviewed)
- `ARCH-006 forbid(unsafe_code) by default`: wega-network does not declare #![forbid(unsafe_code)] (legacy crate; documented exception/not yet reviewed)
- `ARCH-006 forbid(unsafe_code) by default`: wega-platform does not declare #![forbid(unsafe_code)] (legacy crate; documented exception/not yet reviewed)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `anyrender` (normal)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `anyrender_skia` (normal)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `blitz-dom` (normal)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `blitz-html` (normal)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `blitz-paint` (normal)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `blitz-traits` (normal)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `deno_core` (normal)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `deno_core` (build)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `html5ever` (normal)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `skia-safe` (normal)
- `ARCH-007 third-party engines only in adapters (future target)`: wega-engine depends directly on `stylo` (normal)

