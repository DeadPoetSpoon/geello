# WMTS prewarming validation — 2026-11-16

Current configuration uses `wmts_prewarm_size = 5`: a centered 5×5 window,
independent of the 4×4 render metatile. Cached B requests extend their own
window. Shared cache keys contain no user identity; background overlaps join
existing render/encode flights. The 4×4 numbers below record the earlier
aligned-group implementation and are historical, not 5×5 measurements.


Configuration: GPU backend, 4×4 metatiles with 128 px buffer, 4 GiB Foyer
memory, 32 GiB disk, four flush workers, active_background=4.
The flush pool and submit threshold are each capped at 64 MiB, independently
of response retention; all reservations participate in max_live_bytes.

Successful protocol tiles launch bounded neighbor encoding from the same
immutable rendered pixels. Cache lookup and encoding share one singleflight
for foreground/background requests. Foreground subscribers promote queued
background encodes. The group checks captured resource versions, listens for
catalog changes, and drains on shutdown. Only complete successful envelopes
are inserted. Each crop retains the existing version/backend/view/time key.
Cached foreground tiles return immediately and submit a bounded check of their own
centered neighbors. Background workers do not recursively start new windows.
Prewarming is opportunistic when the background queue is full.

## Local GPU measurements

Data: original jd.tif with LXMC polygon outlines, WebMercatorQuad tile
14/13725/7064. New animation time values bypassed completed responses;
decoded blocks and OS caches could be reused. Times include local HTTP.

| Measurement | Result |
| --- | --- |
| First tile after server restart | 2,562.35 ms |
| Entire metatile region renders | 1 |
| Entire metatile tile encodes | 16 |
| Subsequent 15 neighbors | 15/15 Foyer hits |
| Neighbor response range, up to 8 concurrent | 2.09–7.89 ms |
| Extra renders / encodes for those neighbors | 0 / 0 |
| Eight concurrent cold-response requests, same metatile | 2,246–2,251 ms each |
| Extra region renders / total tile encodes for concurrent group | 1 / 16 |
| Foyer memory after first group | 682,617 bytes |
| Foyer memory after all browser and timing checks | 6,863,707 bytes |
| Disk allocated / configured | 34,359,738,368 / 34,359,738,368 bytes |
| Cache errors | 0 |

Three alternating repetitions compared the basic route (no prewarming) with
WMTS. Basic: 2721.84, 2073.99, 1830.61 ms; WMTS: 1962.63, 1838.85,
1847.81 ms. Medians were 2073.99 and 1847.81 ms. No foreground delay was
observed in this sample. This small warm-storage sample is not a guarantee
under arbitrary CPU/storage contention. Separate scheduler admission and
priority promotion protect foreground queue capacity.
Foyer preallocates the full disk device; allocated bytes are not the amount
of live PNG payload. memory_used_bytes reports the memory tier's weighted
entries, not process RSS. prewarm_completed counts successful neighbor
completions, including cache hits and joins of existing foreground flights.

## OpenLayers browser checks

Edge headless, 1440×1100. Both scenarios completed without JavaScript errors
or failed service responses; health continued to report backend=gpu.

| Map / grid | Loaded tiles | Hits added by one-tile pan | Added renders / encodes |
| --- | --- | --- | --- |
| preview:jd-annotations / WebMercatorQuad | 24 | 4 | 0 / 0 |
| preview:combined / WorldCRS84Quad | 18 | 3 | 0 / 0 |

Screenshots and raw measurements are saved locally under target/:
wmts-prewarm-WebMercatorQuad.png, wmts-prewarm-WorldCRS84Quad.png,
wmts-performance.json, wmts-latency-comparison.json and wmts-browser-results.json.

## Automated coverage

- Neighbor enumeration, z=0/1/2/4/30, finite matrix edges, both grids.
- Four concurrent foreground calls including duplicate coordinates; one
  region render and exactly 16 encodes per metatile.
- Neighbor Foyer hits and disk recovery without rendering after restart.
- Background slots occupied: foreground neighbor promotion still completes.
- Replacement cancels old queued prewarms and produces a new revision.
- Foreground encoding failure creates no neighbor work or cached response.
- Shutdown cancels and drains pending prewarms.
- Existing persistent cache recovery/checksum rejection, WMS, WFS and Maps
  HTTP contracts remain covered.

Run: cargo test --features server-full --lib --test server_contracts
--test http_contracts --test derived_contracts

## Centered 5×5 update

Automated tests cover A=(6,6), B=(5,6) at z=4 on both matrix sets. A warms
25 PNGs. B is a cache hit and adds exactly 5 PNGs in x=3, with overlap reused;
those five tiles touch two additional aligned render areas. Repeated B
requests produce no extra renders/encodes. Concurrent A/B/B requests encode
exactly the 30 tiles in the union. Disk recovery, queued foreground promotion,
resource replacement, failure and shutdown checks remain covered.

Live GPU check of the updated service: A=14/13725/7064 returned in 3032.56 ms;
its completed window contained 25 PNGs (4 aligned rendered areas). Two
simultaneous users requested B=14/13724/7064 in 4.15 and 3.69 ms. B added
exactly five PNGs, taking encodes from 25 to 30; region renders stayed at 4.
Health reported GPU and zero cache failures. Raw result:
`target/wmts-centered-validation.json`.

The centered-window validation above predates the capacity regression below.

## Continuous browsing capacity regression

Rapidly replacing WMTS views exposed a gap in the earlier single-window and
batch checks. Each centered 5×5 window independently started
`active_background` tiles. With multiple windows alive, the resulting work
could fill the shared area-render singleflight limit before the scheduler's
separate foreground/background queues were reached. Foreground requests then
returned HTTP 503 (`Area render capacity exhausted`). The WMTS XML error
response hid that detail behind `Request failed`.

All windows now share one tile-worker semaphore of `active_background` permits.
Waiting windows do not register encoding or area flights until they acquire a
permit. Area-flight admission also counts foreground and background work
separately, matching encoding-flight admission. Existing cache joins, foreground
promotion, resource replacement and shutdown behavior remain unchanged.

Regression tests hold every background scheduler worker busy while six fresh
windows are submitted, and verify that only the global background tile budget
is registered. A separate test fills background area admission and verifies
that an unrelated foreground tile still succeeds. These limits prevent
background prewarming from consuming foreground admission; they do not promise
success under arbitrary foreground overload or exhausted memory.

Local GPU verification on November 22, 2026 used `open:overlay` in headless
Chromium at 1920×1080. Eight integer animation-time updates, 1.1 seconds apart,
created fresh WMTS views without reusing completed time-keyed responses. The
old service returned eight HTTP 503 responses out of 55 service responses and
left the viewer in its failed state. Repeating the same sequence after the fix
produced 60 service responses with zero HTTP or JavaScript errors; the viewer
finished with 28 loaded WMTS tiles. Readiness still reported `backend=gpu` and
zero cache failures. This is a reproduction check, not a throughput benchmark.

The library, server and HTTP contract suite passed 87 tests, with four opt-in
tests ignored. The open-data live check also passed all 31 checks through the
running GPU server and preview proxy. Run it with:

```text
python -B tools/publish_open_data_demo.py --verify-only --require-gpu --preview http://127.0.0.1:8080
```
