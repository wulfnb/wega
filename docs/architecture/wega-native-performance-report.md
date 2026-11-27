# Native performance and stability evidence

No whole-browser speedup is claimed. Correctness and security take precedence.
The independent owner is not connected to production V8/CSSOM/resource/control
state, so its isolated measurements cannot stand in for browser performance.

## Workloads and method

The retained comparable-work release benchmark uses the same HTML, viewport,
actual glyph content, child-frame scene and geometry/pixel acceptance on native
and reference backends. Five warmups precede 50 interleaved samples per backend.
Its DOM construction still instantiates the shared Blitz-backed owner on both
sides. Shaping is included in layout; reported shaping subsets must not be added
again. Scene/raster/scroll observations use equivalent work, not an empty scene.
The unchanged gate refuses to benchmark mismatched pixels or geometry.

The new independent-owner release benchmark constructs one Wega document with
200 content elements (actual stored-node count is reported), parses 201 rules,
resolves styles/builds boxes, lays out and shapes real text, mutates an inline
width and restyles/relayouts, builds a display list, records/rasterizes a scene,
and records a scrolled scene. Five warmups precede 50 samples. It asserts real
recorded glyph work, the mutated width and actual red raster pixels. It exercises
no V8 or resource loading and is not directly comparable to the smaller retained
native/reference fixture. It does not measure public-page performance.

The existing release 200-element stage profile also measures cached geometry
queries, CSS processing, mutation relayout and scroll recording (10 samples).
Observations do not introduce timing assertions or change fixture deadlines.

All final release commands, samples and percentiles (p50/p95/p99 where sampled)
are archived in the machine evidence. Build/test process RSS and isolated binary
RSS must be distinguished; neither is per-backend allocation or GPU memory.
Cold browser-process startup, allocation counts, GPU memory, independently
repeated confidence intervals and long-duration public browsing are unmeasured.

## Remaining bottlenecks and limits

- Production HTML/DOM construction and CSSOM/resource/control state remain
  coupled to the reference document, despite independent rendering algorithms.
- Mutation invalidation can rebuild the full native style/box/layout tree; no
  new incremental restyle system or shaping cache was introduced here.
- Selector matching scans authored rules. The new owner currently searches IDs
  in connected tree order, with no maintained ID index; it is not a measured
  performance optimization over the existing owner.
- Intrinsic flex/inline measurements can shape text repeatedly. The confirmed
  correctness fixes take precedence over speculative shaping reuse.
- Resource decode, font arrival, geometry caching and public loading states make
  general browsing substantially different from these bounded fixtures.

The historical baseline had slower native cold style/layout, mutation relayout
and rasterization on its fixture, with faster warm/scroll scene recording.
Those historical numbers are not results of this implementation. Final measured
values belong to their exact archived command and implementation revision.

The flat owner handles a 10,000-level storage clone/drop stress test on a
128 KiB stack. That does not raise browser depth acceptance: native layout still
uses the original 128-level policy, parser budgets remain, and the historical
reference deep-layout acceptance still requires the documented 8 MiB stack.

## Final isolated release measurements

Host: Apple M4, 10 cores, 16 GiB RAM, macOS 27.2.1 arm64, Rust 1.98.0.
Campaign workloads ran sequentially; background OS activity and thermal state
were not controlled as a laboratory experiment. All table values are milliseconds.
The isolated binaries run after compilation, separating RSS from build memory.

Comparable fixture: 44 actual glyphs per backend, identical pixels, zero geometry
error, 50 interleaved samples each after five warmups. Both still use the shared
DOM adapter for document construction.

| Stage | Native p50 / p95 / p99 | Reference p50 / p95 / p99 |
|---|---:|---:|
| DOM construction | 27.693959 / 28.792000 / 29.478834 | 27.759292 / 29.255584 / 29.351333 |
| Cold style/layout | 0.250334 / 0.336375 / 0.357334 | 0.247667 / 0.305834 / 0.332125 |
| Mutation relayout | 0.101541 / 0.128500 / 0.132916 | 0.067083 / 0.089208 / 0.091042 |
| Warm scene recording | 0.009958 / 0.014875 / 0.015167 | 0.017125 / 0.025250 / 0.030416 |
| Raster | 0.103541 / 0.121875 / 0.127625 | 0.086583 / 0.099792 / 0.103750 |
| Scroll recording | 0.005208 / 0.007042 / 0.007291 | 0.007208 / 0.009542 / 0.010250 |

Native remains slower on this fixture in cold style/layout, mutation relayout
and rasterization, while warm/scroll recording is faster. These are observations
on this fixture and run, not an overall improvement claim or an isolated before/after
measurement of this campaign.

Independent fixture: 200 elements, 404 stored nodes, 201 rules, 50 samples after
five warmups. This is a different workload with fewer browser responsibilities;
do not compare its DOM construction time to the shared-adapter fixture.

| Stage | p50 / p95 / p99 |
|---|---:|
| HTML/DOM construction | 0.393416 / 0.417917 / 0.423541 |
| CSS parsing | 0.184042 / 0.195792 / 0.205416 |
| Style/box construction | 3.438500 / 3.533125 / 3.604709 |
| Layout including shaping | 1.559459 / 1.609334 / 1.664541 |
| Shaping subset (included above) | 1.483702 / 1.526580 / 1.554578 |
| Mutation restyle/relayout | 5.032708 / 5.166458 / 5.242708 |
| Display-list recording | 0.054875 / 0.065416 / 0.069167 |
| Scene recording | 0.101583 / 0.113291 / 0.114875 |
| Raster | 0.303958 / 0.318042 / 0.324500 |
| Scroll recording | 0.170375 / 0.177000 / 0.180625 |

Isolated maximum process RSS:

- combined native/reference benchmark: 53,542,912 bytes.
- native 200-element stage profile: 29,360,128 bytes.
- independent owner/pipeline benchmark: 30,277,632 bytes.

These are process peaks, not per-backend allocation counts. The first restricted
`time -l` attempts ran the tests successfully but could not read `kern.clockrate`;
those wrapper failures are retained separately, and the approved host retry
provided the RSS values above. Browser/signed-helper policies were not altered.

[Comparable raw measurements](baseline/blitz-removal-2026-11-17/isolated-release/native_benchmark/benchmark-release.json),
[independent raw measurements](baseline/blitz-removal-2026-11-17/isolated-release/benchmark/independent-benchmark.json),
[stage profile](baseline/blitz-removal-2026-11-17/isolated-release/native_stage_profile/native-stage-profile.json),
and [isolated commands/RSS/binary hashes](baseline/blitz-removal-2026-11-17/isolated-release-results.json)
retain full precision. Cargo-wrapper build/test measurements are also retained
under `release` and `release-independent`, with their command logs.
