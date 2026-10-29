# Performance checks

Run the offline benchmark in release mode:

```sh
cargo run --release -p soapstone-store --example performance -- --check
```

The fixture is generated in memory: exactly 5 MiB, 500 operations and 40 string fields per operation, padded with WSDL
documentation. It uses no customer data, network, filesystem database or system keychain. Parsing runs five times;
sample generation runs 2,500 times; cached request creation and reads run 100 times. `--check` enforces a 2-second
maximum parse, 20-ms maximum sample generation and 50-ms maximum cached creation/read. CI runs this on all three desktop
platforms. These are synthetic regression budgets, not guarantees for arbitrary WSDLs.

Local macOS arm64 measurement, 2026-10-29, optimized release build:

| Operation | Median | Maximum |
| --- | ---: | ---: |
| Parse WSDL | 14.60 ms | 18.72 ms |
| Generate one sample | — | 0.12 ms |
| Create cached request | 0.286 ms | 0.809 ms |
| Read saved request | 0.010 ms | 0.039 ms |

Import including generated/stored samples took 60.47 ms. First compilation time is excluded.

The editor loads on demand. The initial minified JavaScript bundle measured 258.58 kB; the editor/request chunk measured
446.76 kB. A test sequence loads a separate 5.49-kB chunk. Bundle sizes do not measure native launch time.

## Desktop measurements

The native app prints `metric startup: … ms` after project loading and `metric switch: … ms` when the selected request
renders. Run the release executable from a terminal to capture these; use a representative project and repeat both
cold launches and warm navigation. Check startup below 1 second and navigation below 50 ms on the target machine.
Measure the process's resident memory with Activity Monitor (macOS), Task Manager (Windows), or the platform profiler.

Native startup, WebView paint, desktop memory and a direct SoapUI comparison have not been benchmarked by the offline
check. Keep these results separate from Rust timings and record hardware, OS, project size and cold/warm state when
reporting them. The first launch after upgrading an old database also includes offline sample-cache migration.

## Memory and responsiveness

- New requests reuse stored samples instead of reparsing every WSDL/XSD.
- Only the selected response tab mounts a CodeMirror editor.
- Formatting is not persisted as a second body. Body automatically formats XML of any supported response size on a Rust
  background worker and reuses the result across tab changes; Raw and Original preserve the received text.
- Response reads stop at 32 MiB. History keeps at most 20 executions and 64 MiB per request, retaining the latest.
- Comparison output is bounded to 250 removed and 250 added lines, with a 2,000-character limit per displayed line.
