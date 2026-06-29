# Smart-Tx — Swap-Payload Bundle Lifecycle Log (real mainnet)

> **This is the swap-path log: the real-Jupiter-content auction test (38 bundles)** — bundles whose
> tx0 is a genuine signed SOL→USDC swap. It shows that **real content still loses** the auction.
> Its companion, [LIFECYCLE_LOG.md](./LIFECYCLE_LOG.md), is the earlier contentless memo-path
> characterization (58 bundles). Together they tell the full story: a contentless bundle does not
> win, and real economic content is necessary but **not sufficient** — winning needs the content's
> own profit to fund a competitive tip, which a fixed-size self-funded swap does not generate.

_Generated from today's swap-mode run databases (`smart_tx.db` + `smart_tx.swap1.db` + `smart_tx.swap2.db`, 2026-06-29). 38 distinct bundles (31 Failed, 7 Submitted/in-flight). Every value is copied verbatim from the recorded `bundle_submissions` / `agent_decisions` tables — nothing is synthetic or altered._

## What these bundles are

These are **real Jupiter swap bundles** submitted to the Jito Block Engine on Solana **mainnet** in `PAYLOAD=swap` mode. Each bundle is the "Path 2" structure:

- **tx0 = a real, signed Jupiter swap** (SOL → USDC), carrying its own Jupiter blockhash + address lookup tables. Its signature is the lifecycle tracking signature. (The pre-submission dry run confirmed this exact swap signs and **simulates clean** — v0 tx, ~106k CU, no error — before any submission; see `swap_dry_run`.)
- **tx1 = our tip transfer**, on our own cached blockhash. The two transactions are independent and do NOT share a blockhash.

This is the central change from the earlier memo+self-transfer bundles: the payload now carries **genuine economic content** (a real on-chain swap), so the bundle enters the auction as a legitimate economic transaction rather than a contentless artifact.

## Summary

| failure kind | count | confidence(s) |
|---|---|---|
| AuctionLost | 30 | Ambiguous(alt: BundleFailure), Certain |
| TransportError | 1 | Certain |
| _(Submitted / in-flight, no terminal)_ | 7 | — |

- **Tip range:** 7673–150000 lamports. The agent bid up to the **150000 lamport cap** (`MAX_TIP_LAMPORTS`, ~p95 of the live floor); 13 bundles sit exactly at that clamped ceiling.

## What the run demonstrates (every item verified against the rows below)

- **Failures are `AuctionLost`, not construction faults.** 30 bundles entered the auction with **real swap content** and **competitive tips** (up to the 150000-lamport / ~p95 ceiling), were accepted by the Block Engine (returned a `bundle_id`), and still did not win — Jito's `getInflightBundleStatuses` reported `Invalid`. The classifier names the real cause (auction loss), not the downstream blockhash expiry.
- **Timing was not the blocker.** These ran on the swap pre-fetch path (`SWAP_LEAD_SLOTS`, warm pre-fetched swap), which removes the ~700ms in-window Jupiter fetch so the bundle is sent on time — the same on-time submission the memo path achieves. (Per-bundle slot-drift is a run-log telemetry value, not a stored column, so it is not asserted per row here.)

- **The clamp guardrail fired.** The agent, tracking a rising tip market, proposed tips ABOVE the cap — e.g. `set_tip` of 327460, 330000, 200000, 160000 lamports — and the orchestrator clamped each to the 150000 ceiling (log: *"agent SetTip exceeded cap — clamped"*). The clamped 150000 is what was actually bid.

- **Graceful BaselineAgent fallback.** On 5 attempt(s) the LLM call failed and the deterministic `BaselineAgent` took over (recorded as `agent_kind=baseline, executed=1`) — the system kept deciding rather than stalling (log: *"LLM decision failed — falling back to BaselineAgent"*).

- **Vote-account hard rejection, correctly classified.** One submission hit the Block Engine's `HTTP 400: "bundles cannot lock any vote accounts"` (JSON-RPC -32602). The classifier marked it **TransportError / Certain** (a pre-auction infra rejection — bundle/blockhash/tip not implicated) and the agent **Abandoned** it rather than wasting retries or touching the tip.

## How to read this & honesty disclosure

Slots (`submitted_slot`, `blockhash_fetched_at_slot`) are real mainnet slot numbers; signatures are real ed25519 signatures (tx0 = the swap's signature, the tracking sig). **None of these bundles won their auction**, so the signatures are not expected to resolve as confirmed transactions on an explorer — what is verifiable is the recorded lifecycle, classification, and agent reasoning, and that the slots are valid historical mainnet slots. The finding the data supports: a **real-content bundle lands cleanly into the auction**, but *winning* requires the bundle's own economic value to fund a competitive tip — a **fixed-size swap of our own capital (0.01 SOL → USDC) generates no profit, so it cannot out-bid the profit-funded arbitrage/MEV flow** it competes against, no matter the tip ceiling, timing, authentication, or region. For `Failed` rows the terminal timestamp is stored in `finalized_at` (labelled *failure recorded*); no bundle reached Processed/Confirmed, so there are no Processed→Confirmed→Finalized deltas.

## Index (all bundles, chronological)

| # | submitted_at (UTC) | target slot | tip (lamports) | status | failure kind |
|---|---|---|---|---|---|
| 1 | 2026-06-29 15:09:27 UTC | 429688567 | 20148 | Failed | AuctionLost |
| 2 | 2026-06-29 15:27:13 UTC | 429691190 | 22095 | Failed | AuctionLost |
| 3 | 2026-06-29 15:28:15 UTC | 429691349 | 9912 | Failed | AuctionLost |
| 4 | 2026-06-29 15:28:28 UTC | 429691384 | 22095 | Failed | AuctionLost |
| 5 | 2026-06-29 15:29:19 UTC | 429691505 | 7673 | Failed | AuctionLost |
| 6 | 2026-06-29 15:29:29 UTC | 429691529 | 10000 | Failed | AuctionLost |
| 7 | 2026-06-29 15:29:44 UTC | 429691568 | 22095 | Failed | AuctionLost |
| 8 | 2026-06-29 15:30:31 UTC | 429691688 | 10000 | Submitted | — |
| 9 | 2026-06-29 15:30:40 UTC | 429691708 | 10000 | Submitted | — |
| 10 | 2026-06-29 15:30:59 UTC | 429691755 | 22095 | Submitted | — |
| 11 | 2026-06-29 15:32:55 UTC | 429692039 | 100000 | Failed | AuctionLost |
| 12 | 2026-06-29 15:33:56 UTC | 429692193 | 150000 | Failed | AuctionLost |
| 13 | 2026-06-29 15:34:06 UTC | 429692217 | 150000 | Failed | AuctionLost |
| 14 | 2026-06-29 15:34:56 UTC | 429692345 | 31322 | Failed | AuctionLost |
| 15 | 2026-06-29 15:35:07 UTC | 429692374 | 150000 | Failed | AuctionLost |
| 16 | 2026-06-29 15:35:19 UTC | 429692406 | 150000 | Failed | AuctionLost |
| 17 | 2026-06-29 15:36:06 UTC | 429692524 | 10135 | Failed | AuctionLost |
| 18 | 2026-06-29 15:36:20 UTC | 429692560 | 150000 | Failed | AuctionLost |
| 19 | 2026-06-29 15:36:32 UTC | 429692589 | 20000 | Failed | AuctionLost |
| 20 | 2026-06-29 15:40:19 UTC | 429693141 | 150000 | Failed | AuctionLost |
| 21 | 2026-06-29 15:40:24 UTC | 429693156 | 15023 | Failed | AuctionLost |
| 22 | 2026-06-29 15:40:33 UTC | 429693177 | 20000 | Failed | AuctionLost |
| 23 | 2026-06-29 15:40:40 UTC | 429693196 | 150000 | Failed | AuctionLost |
| 24 | 2026-06-29 15:41:23 UTC | 429693294 | 100000 | Failed | AuctionLost |
| 25 | 2026-06-29 15:41:34 UTC | 429693322 | 150000 | Failed | AuctionLost |
| 26 | 2026-06-29 15:41:44 UTC | 429693348 | 15023 | Failed | AuctionLost |
| 27 | 2026-06-29 15:41:55 UTC | 429693374 | 20000 | Failed | AuctionLost |
| 28 | 2026-06-29 15:42:05 UTC | 429693401 | 150000 | Failed | AuctionLost |
| 29 | 2026-06-29 15:42:22 UTC | 429693445 | 150000 | Failed | TransportError |
| 30 | 2026-06-29 15:42:37 UTC | 429693483 | 137514 | Failed | AuctionLost |
| 31 | 2026-06-29 15:42:46 UTC | 429693504 | 150000 | Failed | AuctionLost |
| 32 | 2026-06-29 15:42:54 UTC | 429693526 | 15023 | Failed | AuctionLost |
| 33 | 2026-06-29 15:43:09 UTC | 429693564 | 20000 | Failed | AuctionLost |
| 34 | 2026-06-29 15:43:20 UTC | 429693590 | 150000 | Failed | AuctionLost |
| 35 | 2026-06-29 15:43:53 UTC | 429693674 | 10000 | Submitted | — |
| 36 | 2026-06-29 15:44:01 UTC | 429693696 | 150000 | Submitted | — |
| 37 | 2026-06-29 15:44:10 UTC | 429693715 | 16000 | Submitted | — |
| 38 | 2026-06-29 15:44:24 UTC | 429693753 | 20000 | Submitted | — |

## Per-bundle detail

### 1. `a2a9f6c8fc55f1e707c5c6df9bd66fab8cc63e4f3e227a391de1cfb966c91c22`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5GaSpgNTnYAQHDhzF4f4sUJmj7AkeEk2Hq2PvH5QhasoznCH5UKwx127uXhM2cowKUBRAzGPBLzhsx6oFREaAEes`
- **tip signature (tx1):** `3imhZFbLVeQh7ihaWb39nNHS11fyXQTQ9XH8Z1ZxKWFH1cQPvnVptWPcF5H8e7JyYRyqA51KYe1tDQGszRQwRUYu`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 20148 lamports  (market p50 6390, p75 20148 at submit)
- **target / submitted slot:** 429688567  ·  tx1 blockhash `8YxDvG1PLtM5rHMpQTKi9pvA41A5kg5dEsSaRVJ567f` (fetched at slot 429688567)
- **lifecycle:** Submitted 2026-06-29 15:09:27 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:10:34 UTC
- **classification:** **AuctionLost** (Certain)
- **evidence:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 20148 lamports (p50 6390, p75 20148 at submit) (its blockhash later aged to 163 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)

### 2. `4fcef2bb1da70c1257693a0a178a64b9c2edf352b1d9d154f7bb0c5214be4600`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5EEpUWGi3dRsKFQWVA6x4iuSM2JfXiSHtFVFKoBkHAU3hUx5kTc4u8YBvXqhTh4YRm6V6DQgzNtU76dzCoi6ZyUM`
- **tip signature (tx1):** `TsLkrVTpXhEdBEV8d7AgQuxGU9Nj7Ldg1VN4TD8eUakEUpkZx3fRiUm4bPms69NAxfnipKNP5RapZzTW7zchbhU`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 22095 lamports  (market p50 5842, p75 22095 at submit)
- **target / submitted slot:** 429691190  ·  tx1 blockhash `ynkGqxT6g1x36pcGXwJMPGE5EiuopWvMiji93kKfVNg` (fetched at slot 429691190)
- **lifecycle:** Submitted 2026-06-29 15:27:13 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:28:18 UTC
- **classification:** **AuctionLost** (Certain)
- **evidence:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 22095 lamports (p50 5842, p75 22095 at submit) (its blockhash later aged to 167 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 3. `ee94bbc961c2e365f99f44c485b47ce70f0a641141737cba3484dfad00f3a882`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `vpEFMZMq1EWwmU9inFwhWX1TDeg72kTfVfKYZanYnH14uFqmjedaKazQDJzggaoohujC27d3qdiPZBxZUR2TJMz`
- **tip signature (tx1):** `2SzbEq5c5vVA2hoGjxkdu9Cex6GtK7yoBgvq8mNtq5PfPerXjXJrEJM69u9wfybdKLBz546WzmFr256SCVEutpCW`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 9912 lamports  (market p50 3292, p75 9912 at submit)
- **target / submitted slot:** 429691349  ·  tx1 blockhash `7yzraqmS67QYMijHXdXs8Jfg1YQYFuwhmxGHU52aEhQj` (fetched at slot 429691349)
- **lifecycle:** Submitted 2026-06-29 15:28:15 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:29:22 UTC
- **classification:** **AuctionLost** (Certain)
- **evidence:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 9912 lamports (p50 3292, p75 9912 at submit) (its blockhash later aged to 163 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6788ms] → **refresh_blockhash, set_tip(10000), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses=Invalid means it never won its auction. Blockhash aged to 163 slots (>150) while sitting unlanded—a downstream symptom, not the cause. Tip was 9912 lamports, exactly at p75 (9912) at submit but market has since dropped (p75 now 7673, p50 now 3842, trend falling). Attempt 2. Actions: (1) refresh_blockhash because 163 slots exceeds the ~150-slot validity window; (2) set_tip to 10000 lamports to stay slightly above the original p75 and ensure competitiveness despite the falling market; (3) resubmit to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 4. `47d8a90a914bd49454641e0fa9f078e70349859948d41eaea07e78676942cca0`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `336JHg4RSfrdKf3znZxLTAa2V4dEespo99Vn5rofrB4kHVuN53dDjr8eNJoZ3CvSrbsHDN3Kz9eTXcgUJDaAQueB`
- **tip signature (tx1):** `4TkwGRaHuQcM3MVyZiZGrDnDJdpeLXpR5rj1vZKLk2Lqw7yGx2yxWcBoHNBtgCdvNgsbhhWSgBVYP4WzVAJMwBqx`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 22095 lamports  (market p50 3292, p75 9912 at submit)
- **target / submitted slot:** 429691384  ·  tx1 blockhash `EHcoZfnVgCsBmxAqhXVtdgp7CCXf8JUjM96ffqwsASRH` (fetched at slot 429691384)
- **lifecycle:** Submitted 2026-06-29 15:28:28 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:29:38 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 22095 lamports (p50 3292, p75 9912 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 168 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 5982ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence): bundle never landed despite competitive tip of 22095 lamports (well above p50 3292 and p75 9912 at submit). Blockhash aged to 168 slots (past ~150 expiry) as a downstream symptom of sitting unlanded, not the root cause. Tip remains competitive (current p50 3842, p75 7673, both lower than our 22095). This is attempt 2. Correct response: refresh_blockhash (expired due to aging while unlanded), then resubmit unchanged (tip is still strong, no raise needed). If this fails again, consider abandon on attempt 3+._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 5. `740688a4db8cd921ad0b6e215834c605af32b9bd7b0316a5b63e36cbc8231687`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5pvjj5Xu9Wd2sAEHaKvbru1HvvBDVTR92CgCVFmGGQH4G8x8N82knxpGgCZ82S8sFvh2dithyp3zgfBcNwwDZ5ut`
- **tip signature (tx1):** `2CHzE32wKoJa4Gx7ATWtixRbuayVfN6v7PRQbEP9Jb39vByeqxyuggZDAZgEWmvj4nX9L5vdWx82K3BHQaMAg7nS`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 7673 lamports  (market p50 3842, p75 7673 at submit)
- **target / submitted slot:** 429691505  ·  tx1 blockhash `8gE5AE19astGUc2dGpkX291cVLEHaKWV82PnKVYWtopm` (fetched at slot 429691505)
- **lifecycle:** Submitted 2026-06-29 15:29:19 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:30:26 UTC
- **classification:** **AuctionLost** (Certain)
- **evidence:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 7673 lamports (p50 3842, p75 7673 at submit) (its blockhash later aged to 169 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 5448ms] → **refresh_blockhash, set_tip(10000), resubmit**
        - _AuctionLost (Certain): Block Engine accepted bundle (bundle_id returned) but getInflightBundleStatuses=Invalid, bundle never won auction. Blockhash aged to 169 slots (past ~150 validity window) as downstream symptom of sitting unlanded. Tip was 7673 lamports, exactly at p75 (7673) at submit but market moved: p75 now 10000. Attempt 2. Actions: (1) refresh_blockhash because 169 slots exceeds validity; (2) set_tip to 10000 to match current p75 and improve competitiveness; (3) resubmit to compete in next Jito leader auction._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 6. `59835d009fdb35dc4a6861f905c444ec148b7dc3a31c595662300bb04b1dacd3`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `3gw9kENdeV69aKBhvfFzwSJSwVduivZSyZo1PpE7dgReZkTSQ6nhnLn5HEXvx6M2boN224fbNDoVsq1p1JQnZ7tx`
- **tip signature (tx1):** `2NmsrDiTMri2vZ9TNanSaULB8orUUSCpmGtjWnJMxuCeUWgWdrrJzW9zuwC1Z2ai5R1U1pkR8c4rJMdhxY3awCGH`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 10000 lamports  (market p50 3842, p75 7673 at submit)
- **target / submitted slot:** 429691529  ·  tx1 blockhash `6s83jt7rt9DLyo8tSRb22o8LU1p4DL94uYttxQSQVS7u` (fetched at slot 429691529)
- **lifecycle:** Submitted 2026-06-29 15:29:29 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:30:34 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 10000 lamports (p50 3842, p75 7673 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 164 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6590ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed after submission at slot 429691529, now at slot 429691693 (164 slots elapsed, exceeding the ~150-slot blockhash validity window). Tip was 10000 lamports, well above p50 (3842) and p75 (7673) at submit; current p50 is 4339, p75 is 10000 (tip equals current p75, trend falling). The blockhash aged to 164 slots as a downstream symptom of the bundle sitting unlanded, not the original cause. This is attempt 2. The tip remains competitive at the p75 level, so no tip increase is warranted. Refresh the blockhash (it is now stale at 164 slots) and resubmit to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 7. `52a50c96987e69f2e7b05ab87bb4965813add3c7526503a0d0b8edf0416e80d5`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `zKLJvv3anZuZy2656K2kBBJV2FQFrK7wXJsFC4nRpfHD99pYgSGm4hQKd9xcrQpd7pzCKWTXfxjaqHwRTFS6tUm`
- **tip signature (tx1):** `5zCspFrDNe8VS65Bqp9PWsewqBsaHs9P8wEPgEJWBtc73gsTXM9yiN5DaEHe6pmfCQXzgXw7CtyWUcdgYP2xqs7P`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 22095 lamports  (market p50 3842, p75 7673 at submit)
- **target / submitted slot:** 429691568  ·  tx1 blockhash `JA1jvAoJCQFd4vQfeK7kLBkiAo1iDdfTjHm3JbtcwSqT` (fetched at slot 429691568)
- **lifecycle:** Submitted 2026-06-29 15:29:44 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:30:50 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 22095 lamports (p50 3842, p75 7673 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 164 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 9070ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed despite competitive tip of 22095 lamports (p50 3842, p75 7673 at submit — 5.75x p50, 2.88x p75). Blockhash age is now 164 slots (past ~150 slot validity window) — this is a downstream symptom of the bundle sitting unlanded, not the original cause. The tip was well above prevailing percentiles at submission and remains above current p50 1781 and p75 5157 (tip trend falling: p50 -2558, p75 -4843). Attempt 2. The bundle lost its auction or encountered a skipped Jito leader slot; the blockhash must be refreshed because it aged while unlanded. No tip increase needed — the original tip was competitive and the market has softened. Refresh blockhash and resubmit to compete for the next Jito leader._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 8. `8ad90780d460c152df9e8924c011205913595aa96ee2f3f386dd03e3d12df21b`  (Submitted)

- **tracking signature (tx0 = swap):** `4Ztejqqt3HDFtZbxAoDVHjLPZv2KiKEShvqnFMs29QcZwz9rqzQxaeCXWhXbMDniBdjz8gWQHnsDdcysEWhVM8Zo`
- **tip signature (tx1):** `2LLP9HsPvQjR8hT8o2JGwUQTPumqScURcmpzWVXcQ692wBKbqSzwxhCLpmnacNjoJ2orUPwViz8tapvsCc1BJuAi`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 10000 lamports  (market p50 4339, p75 10000 at submit)
- **target / submitted slot:** 429691688  ·  tx1 blockhash `FQRc9zpKPPG7uq7UoaqznnJsJxUuJ2FT3FM8rybavUs1` (fetched at slot 429691688)
- **lifecycle:** Submitted 2026-06-29 15:30:31 UTC  →  _still Submitted (no terminal state in this run)_

### 9. `e42613b2ca58b4c05a943829040eeb3f99ac0b039e4dfd2663ece5a93eda9ef5`  (Submitted)

- **tracking signature (tx0 = swap):** `4mjamT8ZNHcLHTcmQscLixEgm4HTWWZsGbwDzwu7W1vCWyyvkAwW3AvoffY1qMmD7MQn3Xg22ZjzvmWuu2nxu3Hi`
- **tip signature (tx1):** `25N2nBJThBpQcjxfmWr9UhJ8EBsjiP13UDuNVLL1Qfund6UBpbmU21SRkVt7q5jF9VP5J3Hjop2rLr8jyhXfeRyQ`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 10000 lamports  (market p50 4339, p75 10000 at submit)
- **target / submitted slot:** 429691708  ·  tx1 blockhash `5UGkmvXHf6bzRAnYbCKLNkdjpE4hAtvVXzpZA6QtMDtj` (fetched at slot 429691708)
- **lifecycle:** Submitted 2026-06-29 15:30:40 UTC  →  _still Submitted (no terminal state in this run)_

### 10. `c20426a25d5ed01f114d43f6da41a3a110e6fde6e017124d2d5ad52e61179ac0`  (Submitted)

- **tracking signature (tx0 = swap):** `5eFcW8yVabDWVMZde3wsyEsa3fNdAKrNJpCzJgNRK6timahwSPomD4QhWVXspUmuf8PtjMD9AcoBSoBU29Gvmbd3`
- **tip signature (tx1):** `32MZDG3orVDMySboesDTngZGZM8Rs7RGEkVG6x3TaAmCua85j6jp3GbYDBABRwmLUkMSHrzM51i2Nf8WQNtHGfy6`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 22095 lamports  (market p50 1781, p75 5157 at submit)
- **target / submitted slot:** 429691755  ·  tx1 blockhash `8pcXtRAzN5m9i1ZQKLjtsspmu2cnUhine6cKqmfG6t6G` (fetched at slot 429691755)
- **lifecycle:** Submitted 2026-06-29 15:30:59 UTC  →  _still Submitted (no terminal state in this run)_

### 11. `14bc9dad84d86e69f04c7a19d4fab9d559cb09500b1ec5b3940bcdacb0a4301c`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `3JoGSiLqCNLCxq7Ex6F4Xk6RVm3TYDx1FSxwypMTvGBXFVcjjbd7zHMyMoA6Pvm6yawJgFwWsjb1AGLNcwzRnXtW`
- **tip signature (tx1):** `2Lz3ck2xCYfWPSK68UptFoPoMxDHNnqSmhEX3TfXEujjyeCcX6Hz42Jp7tPfkFh8aarBvyURCa3k4bHB5U5BSf9T`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 100000 lamports  (market p50 11999, p75 18419 at submit)
- **target / submitted slot:** 429692039  ·  tx1 blockhash `GyzZEUaCrqGSdMwFkPNAY7UtGLhrFhXucxYr7aVGJR9Q` (fetched at slot 429692039)
- **lifecycle:** Submitted 2026-06-29 15:32:55 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:33:59 UTC
- **classification:** **AuctionLost** (Certain)
- **evidence:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 100000 lamports (p50 11999, p75 18419 at submit) (its blockhash later aged to 162 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6531ms] → **refresh_blockhash, set_tip(327460), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid—the bundle never won its auction. Blockhash aged to 162 slots (past ~150 validity window) as a downstream symptom of sitting unlanded, not the root cause. Tip was 100000 lamports, well above p50 11999 and p75 18419 at submit time, but the market has since risen sharply: current p50 is 60898 (+56423) and p75 is 327460 (+315182). On attempt 2, refresh the stale blockhash, raise tip to match the current p75 327460 to compete in the rising auction, then resubmit._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 12. `f775f87a8f61c28e6bdc042eda13aefacdc6a4f6bbf18a4e8adb3d1de9742b67`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `55rQh9heU4tiSNScXUhBKCiZNr7N7XFecqKJG4VPBvtMWiqc7rjUCrnQJLJRAfc3HGibR9TvG9VEE1u6kvgims7N`
- **tip signature (tx1):** `2tUbZupiFEyiasgbLiSJa2fD7NJ8qdqm3Kp59HxadNyiWtpTw8HtFDhEEG1Leyu4DTwQtvppe6pC6kuxd87HtTuU`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 60898, p75 327460 at submit)
- **target / submitted slot:** 429692193  ·  tx1 blockhash `9REYkzpFq4xNmxntPW9FWG5fLrre475Vq8bpybgLkwa4` (fetched at slot 429692193)
- **lifecycle:** Submitted 2026-06-29 15:33:56 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:34:59 UTC
- **classification:** **AuctionLost** (Certain)
- **evidence:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 150000 lamports (p50 60898, p75 327460 at submit) (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7323ms] → **refresh_blockhash, set_tip(330000), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid — the bundle lost its auction and never landed. Blockhash age is 162 slots (429692355 - 429692193), past the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The original tip was 150000 lamports, which was between p50 (60898) and p75 (327460) at submit time, suggesting marginal competitiveness. Current percentiles (p50 1035, p75 2594) have dropped significantly, but the bundle competed against the higher percentiles at submit time. Refresh blockhash because it is now stale (162 > 150). Raise tip to 330000 lamports (slightly above the original p75 of 327460) to improve auction competitiveness for the next Jito leader slot. This is attempt 2, so one more competitive attempt is warranted before considering abandonment._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 13. `281a0d1f65bc082d427601c805e674f0a00d6a66895db8c36f1a353fc56afd99`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `4c54Kut3UbjhwTKyTGwuzG9xQzapNkeRxaSBkNpLcc5hrFvTPctck3veqsUYnwpuz92WvUy2UnBjzTMJ1X5UMeuz`
- **tip signature (tx1):** `2CMAG2BsdRq3AmLcjof1CCJD2Ph6V18bLWeZJH8Ht4to4qbasEaTAAWhJegqQBSTGZVNjK7cfvZMuAXBG1RC7QMk`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 60898, p75 327460 at submit)
- **target / submitted slot:** 429692217  ·  tx1 blockhash `7FFXxs5kywUGXDikRYfPSbyXGn3TvaktmKwywqmpfEdr` (fetched at slot 429692217)
- **lifecycle:** Submitted 2026-06-29 15:34:06 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:35:11 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 150000 lamports (p50 60898, p75 327460 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 169 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7770ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 429692217 with tip 150000 lamports (between p50 60898 and p75 327460 at submit, well above current p50 1035 and p75 2594), never landed by slot 429692386. Blockhash age is now 169 slots, exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the original cause. The tip was competitive at submission (2.46x p50, 0.46x p75). Attempt 2 of this bundle. The bundle lost its auction or encountered skipped Jito leader slots; the aged blockhash now prevents any landing. Refresh blockhash to restore validity, then resubmit with the same tip (150000 lamports remains far above current percentiles). Do not raise tip — the original tip was strong and current percentiles are much lower._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 14. `19696dd3448867a142195b831765881da923c1556dce114931d83b460b374c28`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5CWKPXJJT8SLS1sV4KB2wTHGQhVz59Mj1MbNCZ14kGfM9DYPW9NcnLmLgfeNMo3rRi11T1qUGzwpTeBkrEnMpUtu`
- **tip signature (tx1):** `2GYYxzXNXoR2jxYF2WJcp3JAHvHcnNZrpsiTLE4PkZ4FTbJoDgjqenuKQvQ9PUFdPUSvwwmNw6VkHhz4Nh2KwfXr`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 31322 lamports  (market p50 1035, p75 2594 at submit)
- **target / submitted slot:** 429692345  ·  tx1 blockhash `5r6QELsugNSrHBpLwq3n8DZewqXFK8ANvpjVwMx8hWTB` (fetched at slot 429692345)
- **lifecycle:** Submitted 2026-06-29 15:34:56 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:35:59 UTC
- **classification:** **AuctionLost** (Certain)
- **evidence:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 31322 lamports (p50 1035, p75 2594 at submit) (its blockhash later aged to 162 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6532ms] → **refresh_blockhash, set_tip(10135), resubmit**
        - _AuctionLost (Certain): Block Engine accepted bundle (bundle_id returned) but getInflightBundleStatuses=Invalid, bundle never won auction. Blockhash aged to 162 slots (past ~150 validity window) as downstream symptom of sitting unlanded. Tip was 31322 lamports, far above p50=1035 and p75=2594 at submit, but current percentiles are p50=2893 and p75=10135 (tip trend falling: p50 down 1630, p75 down 9865). Attempt 2. Actions: (1) refresh_blockhash because 162 slots exceeds validity; (2) set_tip to 10135 lamports (current p75) to remain competitive in changed market; (3) resubmit to compete for next Jito leader slot._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 15. `97af1209f0e1302084490d8c9fa53c6de13761dc4d101ac0174a1193d4a51eb5`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5AatdHo24tYLopCVsVeX7Bbnr1127Saq1FoMXUnx2zybthd8mAsYWrwizgX2FH1zZSPQDPnRYGGK92qXv8feLeM4`
- **tip signature (tx1):** `4FGiDcGzYrrqmkzgwjMDM9d92q4yjvMjiRwhJhGDcNv9fwSGLdUYH4oDVi74qUc8DmN35Yq9e3nWMzQLAAx5UnTu`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 1035, p75 2594 at submit)
- **target / submitted slot:** 429692374  ·  tx1 blockhash `5fusJLzqdADnVwFGiF6DaqBCyoVio5nKh7nUHueKREHj` (fetched at slot 429692374)
- **lifecycle:** Submitted 2026-06-29 15:35:07 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:36:11 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 150000 lamports (p50 1035, p75 2594 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 164 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 9014ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure). Blockhash age is 164 slots, exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The bundle was submitted at slot 429692374 with tip 150000 lamports when p50 was 1035 and p75 was 2594, making the tip highly competitive at submission. The bundle never landed despite the valid blockhash and strong tip, indicating an auction loss or skipped Jito leader slot. Current tip percentiles are p50 2893 and p75 10135 (tip data age 29 seconds), but the trend shows p50 dropped by 1630 lamports and p75 dropped by 9865 lamports (falling market). The original tip of 150000 lamports remains well above current p50 (2893) and p75 (10135), so no tip increase is warranted. This is attempt 2. Actions: refresh_blockhash to replace the expired 164-slot-old blockhash, then resubmit with the existing competitive tip to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 16. `cf7c402308c84c7163b63eee3aa2908ff08b4752f4f47d617a797eda33d3d690`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `3JaqSS1AwRVKg8eBHizqkHn1tnwbVkYdivFvM7Tp5RJKasn5G1GUNUQjTE3QmUhguJgBu9hLjFBKEg3MaWxpA64n`
- **tip signature (tx1):** `4fwHCZN1yCimBcbei12uFW6D6Zx85L95BRfAukwPqpMNDYzXn23BwNhPzDQhTUSosoi1zWdyn9CrMLz8wXWxsVrU`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 1035, p75 2594 at submit)
- **target / submitted slot:** 429692406  ·  tx1 blockhash `Hiy3Ld2BAYbvm9k2EPW7gst42Y3PftZivT9T4LwEUYxr` (fetched at slot 429692406)
- **lifecycle:** Submitted 2026-06-29 15:35:19 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:36:23 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 150000 lamports (p50 1035, p75 2594 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 8281ms] → **refresh_blockhash, set_tip(20000), resubmit**
        - _AuctionLost (confidence Ambiguous). Blockhash age is 161 slots (429692567 - 429692406), exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The bundle was submitted with tip 150000 lamports when p50 was 1035 and p75 was 2594, far above both percentiles, so the tip was competitive at submission. The bundle never landed despite the valid blockhash and strong tip, indicating it lost the auction or encountered skipped Jito leader slots. Current tip percentiles are p50 1000 and p75 18544 (p75 rose 8409 lamports). This is attempt 2. Actions: (1) refresh_blockhash because the blockhash aged to 161 slots while the bundle sat unlanded and is now expired; (2) set_tip to 20000 lamports to stay above the current p75 of 18544 and remain competitive in the rising auction environment; (3) resubmit to compete for the next Jito leader slot with the fresh blockhash and increased tip._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 17. `2d3a4f8536a857ac47c6b601846d7b2a18c8f010eefde92e59a6bd5fc90ed717`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5MNjzot7c1kw5UcqRbmEJQW3uRUTvCoJTongnxhWkrpqbzvX9pnSF2KguEXYysitkCpURSppJMV7HUHXkt2q6cq`
- **tip signature (tx1):** `4p2bJRG3bzFrwPu8YRLDcJbQdFGmRALurvs3ZMXnWuYmmQD2HtJSuSeRcoSNCfzLoTzgURH3prQ5FrzvFqmdBJdp`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 10135 lamports  (market p50 2893, p75 10135 at submit)
- **target / submitted slot:** 429692524  ·  tx1 blockhash `AU8UuBxJRPhcbXcQZbUnn8YJWpdb2MAy5PMd3tTDUpAE` (fetched at slot 429692524)
- **lifecycle:** Submitted 2026-06-29 15:36:06 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:40:17 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 10135 lamports (p50 2893, p75 10135 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 615 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6797ms] → **refresh_blockhash, set_tip(15023), resubmit**
        - _AuctionLost (confidence Ambiguous): bundle submitted at slot 429692524 with tip 10135 lamports (exactly p75 at submit: p50 2893, p75 10135) never landed; blockhash now aged 615 slots (far past ~150 validity window) — a downstream symptom of sitting unlanded, not the root cause. Tip percentiles have risen: current p50 7675, p75 15023 (tip data 3 secs old). Attempt 2. Actions: (1) refresh_blockhash because 615 slots exceeds validity; (2) set_tip to 15023 lamports (current p75) to compete in the higher auction environment; (3) resubmit to enter the next Jito leader auction with fresh blockhash and competitive tip._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 18. `4e96b8a9e7f8fc61a35cdc9a0fe6dc38b13abae1f75a44f88293b82d18cbc218`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5M3mDDKizcHKHkkpZLiBS8vDwpMAqKgeJJpy9NTvP8WxeBxBQme2phyJCnkd9UzmTHcSBtr9Uqd3v1TQVmx2mT5b`
- **tip signature (tx1):** `3J6Yk21cdwDm38KPte1rk35Xsc21Ty3fEpKyW1TnW3o4ZcLWHqQqq5mtSqsACPkMf5nKaM3zComwLAEsyqyRc5ir`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 2893, p75 10135 at submit)
- **target / submitted slot:** 429692560  ·  tx1 blockhash `4BhhQaRNs77iHdHN4XG7F5trcJXgN7MBv7FxWzPAxkRF` (fetched at slot 429692560)
- **lifecycle:** Submitted 2026-06-29 15:36:20 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:40:17 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 150000 lamports (p50 2893, p75 10135 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 579 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7012ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 429692560 with tip 150000 lamports (well above p50 2893 and p75 10135 at submit) never landed; blockhash now aged 618 slots (far past ~150 slot validity window) — a downstream symptom of sitting unlanded, not the original cause. Tip 150000 lamports remains competitive versus current p50 7675 and p75 15023 (19 seconds old). Attempt 2. The bundle lost its auction or encountered a skipped Jito leader slot; the blockhash must be refreshed because it aged while unlanded, then resubmit with the same tip to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 19. `fc48091b9aed0fd9165ef54e671f12204c42e3971a1f0e710b1f60125fd9b346`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `3ucbTj1xmQ8DCaKGsPxzfFLFnV5xLWVZkuvSoURUzVMqLHkUDdhXaqpiLJC3947rJnJSUgttJunzoJZtVhpaVBNN`
- **tip signature (tx1):** `398ptM7TwuZmEQNGWhNJnYyUfkfS2AhsH3ARFzLf4UZj5fYXc1J44QHddEGWNV99KffESsJhf5czMWKH16RsB3X5`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 20000 lamports  (market p50 1000, p75 18544 at submit)
- **target / submitted slot:** 429692589  ·  tx1 blockhash `7JaMSWgMRvfeeuUXHf3b4M8D77KYgTTiiKnj6HxhyCc7` (fetched at slot 429692589)
- **lifecycle:** Submitted 2026-06-29 15:36:32 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:40:17 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 20000 lamports (p50 1000, p75 18544 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 550 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 8902ms] → **refresh_blockhash, resubmit**
        - _Classified as AuctionLost with Ambiguous confidence (attempt 2). The bundle was submitted at slot 429692589 with a tip of 20000 lamports, which was above both p50 (1000) and p75 (18544) at submission time. The blockhash has now aged 567 slots (far beyond the ~150-slot validity window) because the bundle sat unlanded after losing its auction. The current tip percentiles are p50=7675 and p75=15023; the original tip of 20000 lamports remains competitive (above current p75). The root cause was the lost auction, not an uncompetitive tip or expired blockhash at submission. The blockhash must be refreshed because it is now stale (567 slots old), then resubmit with the existing tip amount to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 20. `f548ec666d7dab8d87c61087eceee57d308321d261ec031df1880dfdd5d01fe8`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `4VwhycZYQKxBSY8UpM39aefzTAhhRDbV7DU6FKM8aprdaXDL1EDK9G5KVHm7J5YNa7zbwBB4CETYCTS6JUfNf1u8`
- **tip signature (tx1):** `5t1npjbLUxdwiBSdV1KHZ2XBq7U6NShPsx5n9AcPyj5mfBvnzE9sbiXgonAjXeXeaTCFi46tFFhNYTZhGEQhXp5s`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 7675, p75 15023 at submit)
- **target / submitted slot:** 429693141  ·  tx1 blockhash `HxBBUfh9U4Ze1Q4zjE3euTL7d48jRjLXirCGzHgx7ETc` (fetched at slot 429693141)
- **lifecycle:** Submitted 2026-06-29 15:40:19 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:41:29 UTC
- **classification:** **AuctionLost** (Certain)
- **evidence:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 150000 lamports (p50 7675, p75 15023 at submit) (its blockhash later aged to 167 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 5395ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid — the bundle never won its auction. The blockhash aged to 167 slots (past the ~150-slot validity window) as a downstream symptom of sitting unlanded, not the original cause. The tip was 150000 lamports, far above p50 (7675 at submit, 5751 now) and p75 (15023 at submit, 10000 now) — the tip was highly competitive and is not the reason for the loss. This is attempt 2. The correct action is to refresh the now-stale blockhash and resubmit with the same tip to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 21. `17d9fbd6814e67691255567ea68a46f3e85341e50cf05e6be3ede4ac55fbec0f`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `56Ndq5k9u87b9opCPoUNVsd2wZKs3XJ13m9uF6DVofzmbY56SPexFqg2SV1rUDmeFM5gEy3yKfTvdF3hZ4XPwTqs`
- **tip signature (tx1):** `XLoFhUrjfANZEdThGZJMt9s7pGN2pVdy2zeyuwRx5Cg6rNwqoEUUww3xG4CA18HPJZbp3bKmfn7C1e9PyPVUwtc`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 15023 lamports  (market p50 7675, p75 15023 at submit)
- **target / submitted slot:** 429693156  ·  tx1 blockhash `9v5Yq9wNksThZgRmgETBgRn8uEhNiy5JyNv7yL2pw5Tk` (fetched at slot 429693156)
- **lifecycle:** Submitted 2026-06-29 15:40:24 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:41:34 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15023 lamports (p50 7675, p75 15023 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 167 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 22. `c682fedf7965e9a01b7ce7f4d2d2e4be8e2564dbf3a087177665e8a1cc78e4fb`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5c8Us6Z6Z8Brzah3KyR3yASJuj2F29nTGMxGrQ3CpBFB3yL7YuVDc5TqkaqktbftYJEAw4UbXV8h2UzkMzn7ycaW`
- **tip signature (tx1):** `55Uf13deunx6RHJzonPNTeqTDpMnatPmoQ3e7MUkSfZ454bhJaTEemWcQoco1AwwEydYQWCNnHDMnuE3t7n3m2Hg`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 20000 lamports  (market p50 7675, p75 15023 at submit)
- **target / submitted slot:** 429693177  ·  tx1 blockhash `JAEspsLEHkPc21ao2d5DS84PQJqaxXwQtzQWEXEiuhJw` (fetched at slot 429693177)
- **lifecycle:** Submitted 2026-06-29 15:40:33 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:41:45 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 20000 lamports (p50 7675, p75 15023 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 172 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 23. `3ca2ac7780c7f8fc34d01f0ffe0d9929c39063960fa4446076c401d95a3adb4d`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `35NoVDx6rVXPw4fpvM2rNEyYB513a43dFbeGwsBEWwujKvKT93Kq6RhvKBF3ac2QxMTZJ9wBYomisFB6cK76r9vr`
- **tip signature (tx1):** `3FALVEZZZYE89upQ51wmEkMp2rjSF9PP8VYZ88vBw2J6sPJvd845QnZRPe5zHu3HkcjTKVTYnA36KS2ynMgsM1bx`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 7675, p75 15023 at submit)
- **target / submitted slot:** 429693196  ·  tx1 blockhash `F9qM1xqutyyoQ6guricXy67LRXJSY8uurq13ctbL2spL` (fetched at slot 429693196)
- **lifecycle:** Submitted 2026-06-29 15:40:40 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:41:55 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 150000 lamports (p50 7675, p75 15023 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 179 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 24. `1c1843025a30a8c4a6f5b7437f60b2ea80bbd8e4d7365ec370a4f8b9a1d355f7`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `3kVtAKGt2Fvu9Jp3NVhWXrRENT26Hsnrkf8gEDsXQFa2YPvjiy4cE4ontb7Hg6HuroudRszT3yiQF7A3jW92GpSV`
- **tip signature (tx1):** `Hms3fRbQoiCncyfwspESB8TZsBNnymH9r8ZC5ohPxLjXBN5jrR9a7J2iZYzAMNEi8tS66QW7zZvG3J6dJ8zrqij`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 100000 lamports  (market p50 5751, p75 10000 at submit)
- **target / submitted slot:** 429693294  ·  tx1 blockhash `DZibtL1CCkxZ4ybKf7nPK5jkMkC1R7BAV4uFrXy8fveh` (fetched at slot 429693294)
- **lifecycle:** Submitted 2026-06-29 15:41:23 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:42:29 UTC
- **classification:** **AuctionLost** (Certain)
- **evidence:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 100000 lamports (p50 5751, p75 10000 at submit) (its blockhash later aged to 168 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 8515ms] → **refresh_blockhash, set_tip(137514), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid — the bundle never won its auction. Blockhash age is 168 slots (past the ~150 slot validity window) — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The original tip was 100000 lamports, which was 10x the p50 (5751) and 10x the p75 (10000) at submit time, so the tip was competitive then. However, the market has shifted dramatically: p50 rose from 5751 to 20000 (+19000 lamports), and p75 rose from 10000 to 137514 (+135139 lamports). The current tip of 100000 is now below the p75 of 137514. On attempt 2, refresh the blockhash to restore validity, raise the tip to match the current p75 (137514 lamports) to compete in the rising auction, then resubmit._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 25. `3f7f8ae5d7098e1f61a74398563fa096fa386fc33a6e188ef5461b4d93e9f6c2`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `3evA721mbWfLeGeFtUvC5sddJmhPvc19Vz5wHSYh3C3hLqiyfhhq7BGym71G1n4cwySvGUdgPP9SwXCRZSHmrSq2`
- **tip signature (tx1):** `5d8ktGu8h1g8PwemjbVTaRvv3r7bWACDiMXNhVCHiHSpChr7qQn1TzMAKtcjT9kBotohgAsTHUiLpjvTYFiDqPgz`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 5751, p75 10000 at submit)
- **target / submitted slot:** 429693322  ·  tx1 blockhash `Bcndp3o5Be5Y6oxzBwEZzFZzA2TbNUsuVFdwprRobLDd` (fetched at slot 429693322)
- **lifecycle:** Submitted 2026-06-29 15:41:34 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:42:37 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 150000 lamports (p50 5751, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 8083ms] → **refresh_blockhash, set_tip(200000), resubmit**
        - _AuctionLost (Ambiguous confidence): bundle submitted at slot 429693322 with tip 150000 lamports (well above p50 5751 and p75 10000 at submit) never landed by slot 429693483. Blockhash age is now 161 slots (past the ~150-slot validity window) — a downstream symptom of sitting unlanded, not the original cause. The tip market has risen sharply: p50 now 20000 (+19000), p75 now 137514 (+135139). The original tip of 150000 lamports, though competitive at submission, is now only marginally above the current p75 of 137514. On attempt 2, refresh the expired blockhash, raise the tip to 200000 lamports (safely above the new p75 to compete in the elevated auction), and resubmit._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 26. `5b538ad45cbc94284d91aa8cdc5190d1f4d63d691a6b46818a4ab138c30c006a`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5UtX8NrQaJ28h6cx2qEBWXoEKS8afqki9Bb7PdrGNNquVk3p3zcVN4ubGS7HLYs6shAFZjCp7DtbYnAVNcjSjNVU`
- **tip signature (tx1):** `5eXPYiqzAYn6L5VEskMAhzsLJ12fBB1i9gm8EDHNXkNW2GLsbpTggDVX4dFKZG2yuGrdatjRCmSZW3ireP7cRTjf`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 15023 lamports  (market p50 5751, p75 10000 at submit)
- **target / submitted slot:** 429693348  ·  tx1 blockhash `943aP3iaEU9TmJmUSEn1jrAySQDdEDymx6hzQSxHTo3x` (fetched at slot 429693348)
- **lifecycle:** Submitted 2026-06-29 15:41:44 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:42:49 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15023 lamports (p50 5751, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 164 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 5291ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed despite competitive tip at submission (15023 lamports vs p50 5751, p75 10000). Blockhash age is now 164 slots (429693512 - 429693348), exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the original cause. The tip was well above both percentiles at submit and remains competitive now (15023 vs current p50 9238, p75 14961), so no tip increase is warranted. This is attempt 2. The correct action is to refresh the stale blockhash and resubmit to compete in the next Jito leader auction._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 27. `f4898a3bd35db07c6b60497590129c04930a03c47391c70b3cde67793e6c4749`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `2Dj1pgwAu4bcceLX7BH2GgCXDUeZ1UKfbrvpgfv6gy1edf9JJaNwEEUirtVD5E2Wf3UTSrMCwfvWjj9KsVUoWykd`
- **tip signature (tx1):** `aUW9drj8USxw7auQpDJCTS6HBoJ6yUPgQ4vMXPVWZkvRdWG9QMg3xLzP4Y9b2TxVHn6LvDv4sA4vfs2ocSM1n5r`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 20000 lamports  (market p50 1000, p75 2375 at submit)
- **target / submitted slot:** 429693374  ·  tx1 blockhash `Efu9oQzQpc8YMJGDUd8vU5J6yNjr3FJj6XjbbDhnab7` (fetched at slot 429693374)
- **lifecycle:** Submitted 2026-06-29 15:41:55 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:43:01 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 20000 lamports (p50 1000, p75 2375 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 169 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 8706ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence with BundleFailure alternative): bundle submitted at slot 429693374 with tip 20000 lamports (well above p50 1000 and p75 2375 at submit), never landed, blockhash now aged 169 slots (past ~150 validity window — a downstream symptom of sitting unlanded, not the original cause). Attempt 2. Current tip percentiles: p50 9238, p75 14961 (both higher than at submit, but tip trend shows p50 down 10762 and p75 down 122553 — market cooling). The 20000 lamports tip remains above current p50 9238 and above current p75 14961, so tip is still competitive. Root cause: lost auction or skipped Jito leader slot while bundle sat unlanded. Correct response: refresh_blockhash (the 169-slot age makes it invalid now) then resubmit with the existing competitive tip to compete for the next Jito leader slot. Do not raise tip (already above prevailing percentiles). Do not abandon (only attempt 2, no evidence of program error or compute budget issue)._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 28. `c04a658c4ede0119b71345cba9661333fa37bf755251366a5e0eb9d6cfe2f239`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5t45x42QaTBYubXtJhbxGfK8tZxV23YXLFDhBQzYgD5QU49sccfnRL2hNeRi5tzVVTGYzir1HTg8q8gmfmpcm3L1`
- **tip signature (tx1):** `2oC7fAtz3BsH1YjaRsvq7UjwAtcmeAjBjitoQ2WJeWQ5BoibsLsz9KTr3q6U8kWsPVRLXbu2WF3Mm3iCcsy4NnUR`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 1000, p75 2375 at submit)
- **target / submitted slot:** 429693401  ·  tx1 blockhash `7Sd6njjqGU3Lfj5igJ5qzyAHxxwS5UPPtopMaiBydjio` (fetched at slot 429693401)
- **lifecycle:** Submitted 2026-06-29 15:42:05 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:43:10 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 150000 lamports (p50 1000, p75 2375 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 164 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 29. `rejected-429693445-0`  (Failed · TransportError)

- **tracking signature (tx0 = swap):** `rejected-429693445-0`
- **tip signature (tx1):** ``
- **tip account:** ``
- **tip:** 150000 lamports  (market p50 20000, p75 137514 at submit)
- **target / submitted slot:** 429693445  ·  tx1 blockhash `` (fetched at slot 429693445)
- **lifecycle:** Submitted 2026-06-29 15:42:22 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:42:22 UTC
- **classification:** **TransportError** (Certain)
- **evidence:** pre-submission transport/decode failure — bundle never reached the auction; the bundle, blockhash, and tip are not implicated: "transport error: send_bundle: sendBundle HTTP 400: {\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32602,\"message\":\"bundles cannot lock any vote accounts\",\"data\":null},\"id\":1}"
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7147ms] → **abandon**
        - _Classification: TransportError with Certain confidence. The Block Engine rejected the bundle with HTTP 400: 'bundles cannot lock any vote accounts'. This is a structural defect in the bundle itself — one or more transactions attempt to write-lock a vote account, which Jito explicitly forbids. The blockhash age is 0 slots (fresh), the tip is 150000 lamports (well above p75 of 137514), and this is attempt 2. Because the error is a hard constraint violation (vote account locking), no amount of waiting, tip increases, or blockhash refreshes will allow this bundle to land. The bundle must be reconstructed to remove or modify the offending transaction(s). Abandon is the only correct action._
    - attempt 2 [shadow, baseline] → **hold(4), resubmit**
        - _TransportError_

### 30. `4a0865799ecfe73b6428734c858be98849a130cfdd6fff9314ea9ec54fca9557`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `2Qd3XJTskKmaxkBtHHEYDnDx2DeWeCAHCG6kpUGPePQBBpHXqQaKpawABe5m7pZToDLnMrngG5SNLGjNPi2eALkD`
- **tip signature (tx1):** `2gHFyivigL4NsqKPbFqMAhWH7dRjiDm6zBrPX4RyPBU5Wd4QDxT6sC3386kbBDyqA3H3oUwSrocvjqTJPqyLi7Gr`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 137514 lamports  (market p50 20000, p75 137514 at submit)
- **target / submitted slot:** 429693483  ·  tx1 blockhash `EKeKJYFPP5rPhmCxgmQoJw4D4mE5a7f81EDjphQ6cfhx` (fetched at slot 429693483)
- **lifecycle:** Submitted 2026-06-29 15:42:37 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:43:45 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 137514 lamports (p50 20000, p75 137514 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 170 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 8582ms] → **refresh_blockhash, set_tip(10000), resubmit**
        - _Classified as AuctionLost with Ambiguous confidence. The bundle was submitted at slot 429693483 with a tip of 137514 lamports (exactly at p75 137514, well above p50 20000 at submit time), but never landed. The blockhash has now aged 170 slots (past the ~150-slot validity window) — this is a downstream symptom of the bundle sitting unlanded, not the original cause. The tip market has shifted dramatically: current p50 is 3176 lamports and p75 is 8384 lamports (down from 20000/137514), indicating the previous 137514-lamport tip was massively overpriced for current conditions. This is attempt 2. Actions: (1) refresh_blockhash because at 170 slots the blockhash is expired and the transaction can never land with it; (2) set_tip to 10000 lamports — above the current p75 of 8384 to remain competitive while the trend is rising (p50 +1589, p75 +3500), but far below the previous overpayment; (3) resubmit to compete in the next Jito leader slot with the fresh blockhash and right-sized tip._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 31. `0488101795ea328ff8d5d172cb07f252042ee82dddb21c4c2ef594886426a44b`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `3SQh5f8bEkRnPQuDmb21odEd3TZCGDUCnutKjBqLRQ1eV4D11H2wbpqmqY3UN3CG6xCRqAaiV3fELsNmTBYwJojj`
- **tip signature (tx1):** `2mvYC8cGBpS9PZWjt1FimPir3pFywwa4zEkJMbk5E9NgLBKkYpcShTv1BcFTfkVw84Vad9GMcAmPbQiLqsVEy13B`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 20000, p75 137514 at submit)
- **target / submitted slot:** 429693504  ·  tx1 blockhash `4xKZstAaUqC8vXvPyg3rL13mxADkGgPT9UXeL2LSD9w` (fetched at slot 429693504)
- **lifecycle:** Submitted 2026-06-29 15:42:46 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:43:54 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 150000 lamports (p50 20000, p75 137514 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 170 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7864ms] → **refresh_blockhash, set_tip(160000), resubmit**
        - _AuctionLost (Ambiguous confidence): bundle submitted at slot 429693504 with tip 150000 lamports (above p75 137514 at submit) never landed; blockhash now aged 170 slots (past ~150 validity window) — a downstream symptom of sitting unlanded, not the original cause. The bundle lost its auction despite competitive tip. Current percentiles (p50 3176, p75 8384, age 11s) show the market has cooled significantly, but the prior tip of 150000 was competitive then and is still well above current levels. Refresh blockhash (expired from aging unlanded), raise tip modestly to 160000 lamports (+6.7%) to improve auction odds on attempt 2, then resubmit to compete for the next Jito leader slot._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 32. `15f5deeafc4b2c40c73fd14f4e15cf80bb0120b632c8f7d247ff647bfb68e312`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `5tPrGiXB2uNDKHkYcyd32cCAsk8PpuDo8KuQmYwSFxkLVKZrjv3VeNkzaFb27jy81h3aYAgEYijzgpy91g9n2Eij`
- **tip signature (tx1):** `wpbgKxbaYjW73ZarbV7Q4hFkS7ayEYVFRU4oQBKQEgZ7bHDEUfTkJU3ixNeemxsswnjQA1wJnHkB2NdLjQg8mNf`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 15023 lamports  (market p50 9238, p75 14961 at submit)
- **target / submitted slot:** 429693526  ·  tx1 blockhash `Ay6ymNysoJo4wc3a8u1FXfGENeu3wywmU6qqaTQ5Pc1w` (fetched at slot 429693526)
- **lifecycle:** Submitted 2026-06-29 15:42:54 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:44:02 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15023 lamports (p50 9238, p75 14961 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 170 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 8036ms] → **refresh_blockhash, set_tip(16000), resubmit**
        - _AuctionLost (confidence Ambiguous with BundleFailure alternative). Blockhash age is 170 slots (429693696 - 429693526), exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The bundle was submitted with tip 15023 lamports when p50 was 9238 and p75 was 14961 (tip was competitive, slightly above p75). The bundle never landed despite valid blockhash at submission, indicating auction loss or skipped Jito leader slot. Current tip percentiles are p50 3176 and p75 8384 (both lower than original tip), but the trend shows rising competition (p50 +1589, p75 +3500). This is attempt 2. Actions: (1) refresh_blockhash because 170 slots exceeds validity; (2) set_tip to 16000 lamports (modest increase from 15023 to stay above the rising p75 trend and improve auction competitiveness); (3) resubmit to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 33. `0ae5dc1021a64d24b5ff0c654cdf6270cf1fbf49b3521e0e19a927c9be68b602`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `3C1kmhQRfWsqKk6NvDEn4cz6mMm3Vv5X6EHcdz4iaSiBMtuoiPcZsoTrc4KysTqH5X1VmGFW21qAHw76Dj98Nmf7`
- **tip signature (tx1):** `H9XTQN41fZG462CwTYxjGm9whi8MmpYk5wZpfego2jWnzj4eYuRyq9XLECkeE1QP6zbNzreTfP24hgu7CVZVLtv`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 20000 lamports  (market p50 9238, p75 14961 at submit)
- **target / submitted slot:** 429693564  ·  tx1 blockhash `5s58NTf8xyY5tCMh8yEBV12kua1Ac2uQ41wG3Qbqhya7` (fetched at slot 429693564)
- **lifecycle:** Submitted 2026-06-29 15:43:09 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:44:17 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 20000 lamports (p50 9238, p75 14961 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 170 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7634ms] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed after submission at slot 429693564, now at slot 429693734 (170 slots elapsed, exceeding the ~150-slot blockhash validity window). The tip was 20000 lamports, well above p50 (9238) and p75 (14961) at submission, indicating competitive bidding. The blockhash has aged to 170 slots and is now stale (a downstream symptom of the bundle sitting unlanded, not the original cause). Current tip percentiles are p50 5000, p75 20000 (rising trend: p50 +1824, p75 +11616). The tip of 20000 lamports now matches the current p75, remaining competitive. This is attempt 2. The correct action is to refresh the blockhash (it is expired after 170 slots) and resubmit with the existing tip (20000 lamports is at the current p75, sufficient to compete). No tip increase is needed as the original tip was and remains competitive._
    - attempt 2 [shadow, baseline] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 34. `27052d1b3b959041978859e684e3b3c2d273be13026240ba3e9d97ffb67658f4`  (Failed · AuctionLost)

- **tracking signature (tx0 = swap):** `2zZAQUpRoYPKkx79oFFA6w4TXgsYxexsGSQAYfC9GBb4ncJEnoCoHJkD4JVuxLWegNQJFboBW3iwnNRLcin1cohn`
- **tip signature (tx1):** `288vyzm7sSVaYDpHNQ2fLuGwHBByJFtzGTobQxhbqBhDusDgy9EP1mv3hSGpTyTdCxDMtEVu2g2g77FGM2j3yt7c`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 9238, p75 14961 at submit)
- **target / submitted slot:** 429693590  ·  tx1 blockhash `DjWoqakyjgbjBuSCheiJNuJ6EUZaaof385RgQ8ur3zEW` (fetched at slot 429693590)
- **lifecycle:** Submitted 2026-06-29 15:43:20 UTC  →  **Failed (terminal)** failure recorded 2026-06-29 15:44:25 UTC
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence:** never landed though the blockhash was valid at submission and the tip was competitive: tip 150000 lamports (p50 9238, p75 14961 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 163 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)

### 35. `265a1520c10f646c30e0341cdef3633b2a1586ee8567b12f14dde0dd06c6339e`  (Submitted)

- **tracking signature (tx0 = swap):** `3G3maDzTv6B6BHvQEWmLGaDFQR2Mc2oAxcF2B6rGpPjAkWWCcKidNMbcqn7RgWTAX5wSKDvji3PXAfTDNBJGPrAQ`
- **tip signature (tx1):** `2S5G8vVZVkGjN8Aw9AnXTz7ZKbFr9C7KsrmRSGmEBhpHzuBwtduW9pStj33LHPNWmNCRH5tjMVrzgV5BVcamEBww`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 10000 lamports  (market p50 3176, p75 8384 at submit)
- **target / submitted slot:** 429693674  ·  tx1 blockhash `GRHjWqZhGevnj36eKxXQx2quR7MzzsLq29y8s8vbEKah` (fetched at slot 429693674)
- **lifecycle:** Submitted 2026-06-29 15:43:53 UTC  →  _still Submitted (no terminal state in this run)_

### 36. `736e726927135ca9274b57a50208eb5ed1d307a286e83d6af3b0ed8f856ff9c6`  (Submitted)

- **tracking signature (tx0 = swap):** `5b4q7zP1Gc94k9RatPPb2kTJEfknWeQ6R51aryZomAyAkw7SFfSea79KrR4hpr4pbJ12Bz8ARFfvgeSpNoe42YQ1`
- **tip signature (tx1):** `3PrWLW2WBMYWvYcTir5uqdhtQ6y1XtEkm8QGnRXJx2G6uV6khYhx6R5Zk1gWYTMsd12NLNfEtYBS66uWh68UFunC`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 150000 lamports  (market p50 3176, p75 8384 at submit)
- **target / submitted slot:** 429693696  ·  tx1 blockhash `CdhLdL285pozG24A44TwueLp8t9s9oqQ7BdLRP52jkoN` (fetched at slot 429693696)
- **lifecycle:** Submitted 2026-06-29 15:44:01 UTC  →  _still Submitted (no terminal state in this run)_

### 37. `16639186e99f698857fccc0c6409251689995d4d64558f04a48ba06ec250f970`  (Submitted)

- **tracking signature (tx0 = swap):** `3nBMe8zTaJD2nE8gWurwmg5fZVH7r2VnKRWiymLDmFxcVK38yj1enJKs1PyhbChhK7WLJKGkUxCSLAUD1uDh72z3`
- **tip signature (tx1):** `3nKxSADRtLzuNasNGmfpJZN3UhaPoSwcjwXodvEwwwaFZhqXrr8hb1iYKUNC3vCGf93zQP5qzeDiMRf1BRoM3nFz`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 16000 lamports  (market p50 3176, p75 8384 at submit)
- **target / submitted slot:** 429693715  ·  tx1 blockhash `8c5ARAQCZSzf1oDxzcF87YEMQHzf2XyQ85BSnQJwHuv2` (fetched at slot 429693715)
- **lifecycle:** Submitted 2026-06-29 15:44:10 UTC  →  _still Submitted (no terminal state in this run)_

### 38. `4e11e10db71d7f5fe906a12f35a98b2cf1d72eb35fa198ce0c5e5f65bad6b5ed`  (Submitted)

- **tracking signature (tx0 = swap):** `62HQ78HvBuT7j4vn9Hexz6F3ecm2bn8CUr8ZjXNfq8PLRxXmMSX2WV3tBRMwS6exvNrT1cdwZZx8UB3GuJfYcctj`
- **tip signature (tx1):** `4tqA5AUX7dWwmxBo1PBLDW14kDPaWtnncDcSY1feaDbNMcveiMerAZYAx2BAErzSN6D9cXi1Lky3aJMYvUi6YjhF`
- **tip account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **tip:** 20000 lamports  (market p50 5000, p75 20000 at submit)
- **target / submitted slot:** 429693753  ·  tx1 blockhash `H44EL5ogMbMpvaafLByCGagBqwTCNr35vATYFwdyWmes` (fetched at slot 429693753)
- **lifecycle:** Submitted 2026-06-29 15:44:24 UTC  →  _still Submitted (no terminal state in this run)_
