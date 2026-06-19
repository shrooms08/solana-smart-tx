# Smart-Tx — Bundle Lifecycle Log (real mainnet submissions, AuctionLost classifier)

_Generated from `smart_tx.db` — the latest run, recorded AFTER the AuctionLost classifier fix with `jito_inflight_status` now persisted by the bundle-status poller. 58 distinct bundles (49 Failed, 9 Submitted/in-flight), 95 agent decisions. Every value is copied verbatim from the recorded `bundle_submissions` and `agent_decisions` tables — nothing is synthetic or altered._

## How to read this

Each entry is one bundle the stack constructed and submitted to the Jito Block Engine on Solana **mainnet**, with the lifecycle state machine's recorded transitions and (for failures) the classifier's verdict and the retry agent's decisions.

**What this run demonstrates (the headline):**

- **Failures are correctly classified as `AuctionLost`, not expired blockhash.** All 49 terminal bundles are `AuctionLost`: the Block Engine *accepted* each bundle (returned a `bundle_id`), but it never won its auction. The blockhash aging past ~150 slots is recorded as a **downstream symptom of sitting unlanded**, never the cause. This corrects the earlier logic that mislabeled every timeout `ExpiredBlockhash`.
- **`Certain` vs `Ambiguous` confidence.** 10 failures are **`Certain`** — Jito's `getInflightBundleStatuses` was polled and returned **`Invalid`** (persisted in the `jito_inflight_status` column), direct proof the bundle was not in Jito's system / never entered the auction. The other 39 are **`Ambiguous`** (alternative: `BundleFailure`) — the same never-landed-despite-valid-blockhash-and-competitive-tip pattern, but the poller had not captured a definitive `Invalid`/`Failed` before the sweep, so auction loss is inferred rather than proven.
- **The agent is market-adaptive.** The LLM reasons over live tip percentiles (p50/p75) and their trend: it **raises** the tip when the auction is *rising* (e.g. p75 surged since submit), **lowers** it when the market has *cooled* (to stay competitive without overpaying), and distinguishes a competitive-tip-but-stale-blockhash situation (refresh + resubmit) from a needs-higher-bid one (refresh + set_tip + resubmit). Concrete examples are in the per-bundle sections.
- **Graceful fallback / resilience.** On 2026-06-19 07:35:17 UTC the LLM call failed for one attempt and the deterministic `BaselineAgent` transparently took over, choosing a safe `refresh_blockhash + resubmit` — the pipeline never stalls when the model is unavailable (bundle #35, attempt 2).

**Field notes:**

- **Slots** (`submitted_slot`, `blockhash_fetched_at_slot`) are real mainnet slot numbers at submission.
- **Signatures** (`memo_signature`, `tip_signature`) are real ed25519 signatures of the signed transactions; **bundle_id** is the value Jito's `sendBundle` returned.
- **Leader / is_bam:** the leader-window gate only ever targets **Jito-enabled leaders** (a bundle is not submitted otherwise), but the leader identity and BAM flag are **not persisted** in the lifecycle row, so they are omitted per-bundle here rather than fabricated.
- For `Failed` rows the terminal timestamp lives in the `finalized_at` column (the moment the failure was recorded) and is labelled **"failure recorded"** below.

**Honesty disclosure:** none of these bundles **won their auction / landed on-chain**. A contentless memo + tip bundle does not out-compete real economic MEV flow in the auction, which is exactly what `AuctionLost` records. So the signatures will **not** resolve as confirmed transactions on a block explorer; what is verifiable here is the **recorded lifecycle, the classification, the agent's reasoning, the persisted Jito `Invalid` status, and that every slot is a valid historical mainnet slot.** No bundle reached Processed/Confirmed/Finalized, so there are no commitment-transition latency deltas to report.

## Summary

- **Bundles:** 58  ·  **Failures:** 49 (all `AuctionLost`)  ·  **Submitted/in-flight:** 9
- **AuctionLost confidence:** 10 `Certain` (Jito `Invalid` persisted)  ·  39 `Ambiguous` (inferred, alt `BundleFailure`)
- **Persisted Jito `Invalid` statuses:** 10
- **Agent decisions:** 95 total — 47 executed LLM, 47 baseline shadow, 1 **executed baseline fallback**
- **Tip range:** 2,542 – 25,000 lamports
- **Slot span:** 427,460,134 – 427,461,799

| failure kind | confidence | count | meaning |
|---|---|---|---|
| AuctionLost | Certain | 10 | Jito getInflightBundleStatuses returned **Invalid** — proven not in auction |
| AuctionLost | Ambiguous (alt BundleFailure) | 39 | never landed despite valid blockhash + competitive tip — auction loss inferred |
| _(Submitted, in-flight)_ | — | 9 | submitted near run end, never swept to terminal |

## Index (chronological)

| # | submitted_at (UTC) | slot | tip | status | kind | confidence | jito_status |
|---|---|---|---|---|---|---|---|
| 1 | 2026-06-19 07:25:51 UTC | 427460134 | 7312 | Failed | AuctionLost | Certain | Invalid |
| 2 | 2026-06-19 07:26:52 UTC | 427460286 | 10000 | Failed | AuctionLost | Certain | Invalid |
| 3 | 2026-06-19 07:27:04 UTC | 427460316 | 10000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 4 | 2026-06-19 07:27:52 UTC | 427460438 | 10045 | Failed | AuctionLost | Certain | Invalid |
| 5 | 2026-06-19 07:28:05 UTC | 427460469 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 6 | 2026-06-19 07:28:16 UTC | 427460498 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 7 | 2026-06-19 07:28:51 UTC | 427460590 | 2542 | Failed | AuctionLost | Certain | Invalid |
| 8 | 2026-06-19 07:29:03 UTC | 427460623 | 2542 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 9 | 2026-06-19 07:29:17 UTC | 427460655 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 10 | 2026-06-19 07:29:27 UTC | 427460682 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 11 | 2026-06-19 07:29:52 UTC | 427460742 | 15000 | Failed | AuctionLost | Certain | Invalid |
| 12 | 2026-06-19 07:30:05 UTC | 427460773 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 13 | 2026-06-19 07:30:16 UTC | 427460803 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 14 | 2026-06-19 07:30:30 UTC | 427460835 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 15 | 2026-06-19 07:30:38 UTC | 427460856 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 16 | 2026-06-19 07:30:53 UTC | 427460894 | 11415 | Failed | AuctionLost | Certain | Invalid |
| 17 | 2026-06-19 07:31:04 UTC | 427460922 | 17000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 18 | 2026-06-19 07:31:16 UTC | 427460953 | 17000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 19 | 2026-06-19 07:31:28 UTC | 427460984 | 20000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 20 | 2026-06-19 07:31:40 UTC | 427461013 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 21 | 2026-06-19 07:31:53 UTC | 427461045 | 20000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 22 | 2026-06-19 07:31:53 UTC | 427461046 | 10000 | Failed | AuctionLost | Certain | Invalid |
| 23 | 2026-06-19 07:32:03 UTC | 427461069 | 12000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 24 | 2026-06-19 07:32:16 UTC | 427461103 | 17000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 25 | 2026-06-19 07:32:25 UTC | 427461127 | 17000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 26 | 2026-06-19 07:32:44 UTC | 427461175 | 25000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 27 | 2026-06-19 07:32:54 UTC | 427461198 | 10000 | Failed | AuctionLost | Certain | Invalid |
| 28 | 2026-06-19 07:32:54 UTC | 427461200 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 29 | 2026-06-19 07:33:04 UTC | 427461225 | 10000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 30 | 2026-06-19 07:33:13 UTC | 427461246 | 25000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 31 | 2026-06-19 07:33:19 UTC | 427461261 | 12000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 32 | 2026-06-19 07:33:28 UTC | 427461282 | 17000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 33 | 2026-06-19 07:33:40 UTC | 427461312 | 17000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 34 | 2026-06-19 07:33:56 UTC | 427461350 | 2819 | Failed | AuctionLost | Certain | Invalid |
| 35 | 2026-06-19 07:34:00 UTC | 427461362 | 25000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 36 | 2026-06-19 07:34:08 UTC | 427461381 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 37 | 2026-06-19 07:34:15 UTC | 427461398 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 38 | 2026-06-19 07:34:22 UTC | 427461417 | 10000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 39 | 2026-06-19 07:34:30 UTC | 427461438 | 25000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 40 | 2026-06-19 07:34:37 UTC | 427461456 | 12000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 41 | 2026-06-19 07:34:43 UTC | 427461471 | 17000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 42 | 2026-06-19 07:34:50 UTC | 427461488 | 17000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 43 | 2026-06-19 07:34:57 UTC | 427461506 | 8590 | Failed | AuctionLost | Certain | Invalid |
| 44 | 2026-06-19 07:35:06 UTC | 427461530 | 8590 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 45 | 2026-06-19 07:35:17 UTC | 427461555 | 25000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 46 | 2026-06-19 07:35:25 UTC | 427461578 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 47 | 2026-06-19 07:35:32 UTC | 427461593 | 10000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 48 | 2026-06-19 07:35:40 UTC | 427461611 | 15000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 49 | 2026-06-19 07:35:47 UTC | 427461631 | 25000 | Failed | AuctionLost | Ambiguous(alt: BundleFailure) | — |
| 50 | 2026-06-19 07:35:57 UTC | 427461656 | 12000 | Submitted | — | — | — |
| 51 | 2026-06-19 07:36:03 UTC | 427461670 | 17000 | Submitted | — | — | — |
| 52 | 2026-06-19 07:36:10 UTC | 427461687 | 17000 | Submitted | — | — | — |
| 53 | 2026-06-19 07:36:17 UTC | 427461704 | 10000 | Submitted | — | — | — |
| 54 | 2026-06-19 07:36:25 UTC | 427461723 | 8590 | Submitted | — | — | — |
| 55 | 2026-06-19 07:36:33 UTC | 427461744 | 25000 | Submitted | — | — | — |
| 56 | 2026-06-19 07:36:40 UTC | 427461765 | 15000 | Submitted | — | — | — |
| 57 | 2026-06-19 07:36:48 UTC | 427461783 | 10000 | Submitted | — | — | — |
| 58 | 2026-06-19 07:36:54 UTC | 427461799 | 15000 | Submitted | — | — | — |

## Per-bundle detail

### 1. `d964a736bfde62575fd81d567989b4aab925c2c3b9886a13e18c180b59d63c0d`  (Failed · AuctionLost · Certain)

- **memo_signature:** `wk7oSbUrjAptRJFFmD8JbNbMfyc3yBVsa5ArFdFv4cGcCXH71UgPHmSdgNpHqpU95JiJLuC9YwN5P6jxgsCCqLN`
- **tip_signature:** `2zDrQ4faBUZrwpsSFUhgcW4PNhB4i3iYbWHUjWc8BQi8M3oYdeyhTcM13qHBk8epQJKBe8fUBomGNLzuwkQjVCV8`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `ErT3qvsgEox1AxWxKWySEWaprzSDwtxSxXCofZ1VkHDR`  (fetched at slot 427460134)
- **target / submitted slot:** 427460134  (Jito leader; identity/is_bam not persisted)
- **tip:** 7312 lamports  (market p50 1911, p75 7312 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:25:51 UTC  (slot 427460134)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:26:57 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 7312 lamports (p50 1911, p75 7312 at submit) (its blockhash later aged to 164 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6550ms, 2026-06-19 07:27:04 UTC] → **refresh_blockhash, set_tip(10000), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid—the bundle never won its auction. Blockhash aged to 164 slots (past ~150 expiry) as a downstream symptom of sitting unlanded, not the root cause. Tip was 7312 lamports, exactly at p75 (7312) at submit but the bundle still lost. Current p75 is now 10000 lamports. Attempt 2. Actions: (1) refresh_blockhash because 164 slots exceeds validity window, (2) set_tip to 10000 lamports to match current p75 and improve competitiveness, (3) resubmit to compete in next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:27:04 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 2. `1bb9b211f1b0f3aa6f95bf2b3d3fe3a5c5704001f99095cace4f58a69cdea14b`  (Failed · AuctionLost · Certain)

- **memo_signature:** `5HiuMd3gmvNATaexpqWbgGFBfTwWoD81799Q9awUqt7QzZXEXnmmvoMKwRvucmKTuR685SyUmvpxUc7uoVXHHP8a`
- **tip_signature:** `eU8d11m7pGaJyvFwfkFukzddYJPSTZ46cze7hSFFmfGEt5m6LDaVb8NBtezZ4BJ22MPDtqCgBtUBzvcgPHSu1nj`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `DdG15g4g1a8gVN61imLXu7v46eSwJKJPyrknFM7pr2CH`  (fetched at slot 427460286)
- **target / submitted slot:** 427460286  (Jito leader; identity/is_bam not persisted)
- **tip:** 10000 lamports  (market p50 1157, p75 10000 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:26:52 UTC  (slot 427460286)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:27:57 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 10000 lamports (p50 1157, p75 10000 at submit) (its blockhash later aged to 163 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7712ms, 2026-06-19 07:28:05 UTC] → **refresh_blockhash, set_tip(15000), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid—the bundle never won its auction. Blockhash aged to 164 slots (past ~150 expiry) as a downstream symptom of sitting unlanded, not the root cause. Tip was 10000 lamports, exactly at p75 (10000) at submit but the auction was lost. Current percentiles show p50=1730, p75=10045 with rising trend (p75 +8064 lamports). Attempt 2. Actions: refresh_blockhash (164 slots exceeds validity window), set_tip to 15000 lamports (50% above current p75 to compete more aggressively given the lost auction and rising trend), then resubmit to target the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:28:05 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 3. `2ccbad0cd2e1c788e12892f07171ba4648df76b34a110095c9b1490d7f5dfa2f`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `Z1GmhS7vofvddpGDZ1HP6L7LZZraYqqUW1owaSqaXhhabNnedFYsE1oagkjmnK2P1F6Vz2xFQoPkQZXG1J1V6Dc`
- **tip_signature:** `ZSyPkDPPLu9KYnZ8GtD2ZqZKjrNkKtf42dqCJKfrxjAQNsqdXY2AkLAyT9GaJxvd9btD5WqLGVaJbkaXMo8VqWm`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `BjY7mUfb8ueZmM7toafcDND7cVPn91c9iW9JFCiHm2uM`  (fetched at slot 427460316)
- **target / submitted slot:** 427460316  (Jito leader; identity/is_bam not persisted)
- **tip:** 10000 lamports  (market p50 1157, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:27:04 UTC  (slot 427460316)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:28:09 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 10000 lamports (p50 1157, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 165 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6628ms, 2026-06-19 07:28:16 UTC] → **refresh_blockhash, set_tip(15000), resubmit**
        - _AuctionLost (Ambiguous confidence): bundle submitted at slot 427460316 with tip 10000 lamports (exactly p75 at submit: 10000) never landed; blockhash now aged 165 slots (past ~150 validity window) — a downstream symptom of sitting unlanded, not the root cause. Tip was competitive at submission (p50 1157, p75 10000) but market has risen: p50 now 1730 (+729), p75 now 10045 (+8064, +45 lamports above our tip), trend rising over 26 seconds. Attempt 2. Actions: (1) refresh_blockhash to replace the stale 165-slot blockhash, (2) set_tip to 15000 lamports (~50% above current p75 10045) to outbid the rising auction, (3) resubmit to compete for the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:28:16 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 4. `db81140093bbc76fd0360b7fc8a88b955efc53a0542087f5f4cf1317b3131e92`  (Failed · AuctionLost · Certain)

- **memo_signature:** `7teWKmRcNrAN1zyPGrjVxen8AfrS2RB7vGGbEjMD2dNmZUrrC1FEautommtrie6VoZKx1VoHkSMv1QP3jG4twsQ`
- **tip_signature:** `38dxPx41huJC51QsNvCJkaAPZADTsGvAsCnT5nCQHZtBnt4AtogcQDBwi52FUykDzZ8mLvXtTMacMfQic7AtEy2U`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `ACxpPjeNmDqLoDBZz1GEvZ9UX4b1gNVLmNW3YV77oPuC`  (fetched at slot 427460438)
- **target / submitted slot:** 427460438  (Jito leader; identity/is_bam not persisted)
- **tip:** 10045 lamports  (market p50 1730, p75 10045 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:27:52 UTC  (slot 427460438)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:28:57 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 10045 lamports (p50 1730, p75 10045 at submit) (its blockhash later aged to 168 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6345ms, 2026-06-19 07:29:03 UTC] → **refresh_blockhash, set_tip(2542), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid—the bundle never won its auction. Blockhash age is 168 slots (past the ~150-slot validity window)—this is a downstream symptom of the bundle sitting unlanded, not the root cause. The original tip was 10045 lamports (at p75 when submitted), but current p75 is now 2542 lamports and p50 is 1000 lamports—the market has cooled significantly. Attempt 2. Actions: (1) refresh_blockhash to obtain a valid blockhash for the next attempt, (2) set_tip to 2542 lamports to match the current p75 and remain competitive without overpaying in the cooler market, (3) resubmit to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:29:03 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 5. `168f76c28a6a535c3d146027cd4d3f11d5017eb35902bed065c17810c5f0251c`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4kPakrFjQFh8sbfNX3rW6jfLKYk5TT4AxtxVpJRcaSmUx5izAxEK8XmnbwNwUb4FpQvYcxSmLe6XHwugUdM5fT1w`
- **tip_signature:** `vTfBFJ5AdNRtpGhEdC1aTjW3NZ9up1LkFcducjZWbYqQRpjLJArvr4EbbyVTrWYGTVQmtdWTbFeUrf5aQGdd4hB`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `4ungXUkzS6dDdQZUSRM1xvUwZiZ9XhbGBsQ5WyngAXqr`  (fetched at slot 427460469)
- **target / submitted slot:** 427460469  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1730, p75 10045 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:28:05 UTC  (slot 427460469)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:29:09 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1730, p75 10045 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 168 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7482ms, 2026-06-19 07:29:17 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed despite competitive tip of 15000 lamports (p50 1730, p75 10045 at submit). Blockhash age is now 168 slots (427460637 - 427460469), exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the original cause. The tip was well above both percentiles at submission and remains above current p50 1000 and p75 2542 (tip trend shows p75 rising only 458 lamports, not rising overall). Attempt 2 of this bundle. The bundle lost its auction or encountered a skipped Jito leader slot; the aged blockhash now prevents any retry. Refresh the blockhash to make the bundle valid again, then resubmit with the existing 15000-lamport tip (still competitive). Do not raise the tip — it was never the problem._
    - attempt 2 [shadow, baseline, 2026-06-19 07:29:17 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 6. `3017f0f5d33d41056436082570e7a8f17e58b13bde9e97e7fa3924cf116dbcd7`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4YY5ztvdLGzZx7NEmoNwsPXTe4aTsZPnJf2btJoGoXWVjGFX3XXh5bPgGHMjeLdhM43h3ZBcLMiQGkWdFCew8Brn`
- **tip_signature:** `G9rD6ZioQmvM6sxFtnSSmNtKvXkBT5jKRiC2dsvKYaxCgS7BoqefED9T6xov7c2Ka2vJ74w8MfRJfyvoeQnbioU`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `8nTW67vtb8ZhVtfiTejGzJ8vVFwkkmyhtrX9nQwfcQwu`  (fetched at slot 427460498)
- **target / submitted slot:** 427460498  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1730, p75 10045 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:28:16 UTC  (slot 427460498)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:29:21 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1730, p75 10045 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 169 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6383ms, 2026-06-19 07:29:27 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427460498 with tip 15000 lamports (well above p50 1730 and p75 10045 at submit) never landed; blockhash now aged 169 slots (past ~150 validity window) — a downstream symptom of sitting unlanded, not the root cause. Tip 15000 lamports remains highly competitive versus current p50 1497 and p75 4181 (despite rising trend +497/+1639 lamports). Attempt 2. The bundle lost its auction or encountered skipped Jito leader slots; the tip was strong so no raise is warranted. Refresh the stale blockhash and resubmit to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:29:27 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 7. `4aa92d80a1e983682e73526591323417e8b4f105700827ef156d3d46892c5452`  (Failed · AuctionLost · Certain)

- **memo_signature:** `2MgHpYTFe8BBQ7Sj8oLiskk3W5LmNkPovtqjeN7EZ5izAvUhAzoENsTmvyNPiCFpZg6kfKuiQ3bbgFQypWJ6Zp8h`
- **tip_signature:** `RVHbS9EY1XEJppBEmmXPsvrA6exNK4TuAeLLvsb5TDZA3ujQDYsxZxRkCaswvJvWXh998uMfMHSmfsUUcKWDFSu`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `9WvwfzrZ6nYMrEs5K5JeCy2xoZxcQKzeoZzttm5umv9X`  (fetched at slot 427460590)
- **target / submitted slot:** 427460590  (Jito leader; identity/is_bam not persisted)
- **tip:** 2542 lamports  (market p50 1000, p75 2542 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:28:51 UTC  (slot 427460590)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:29:57 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 2542 lamports (p50 1000, p75 2542 at submit) (its blockhash later aged to 163 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7965ms, 2026-06-19 07:30:05 UTC] → **refresh_blockhash, set_tip(15000), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid — the bundle never won its auction. Blockhash age is 163 slots (past the ~150 slot validity window) — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The original tip was 2542 lamports, exactly at p75 (2542) at submit time, but the market has risen sharply: p75 is now 15000 lamports (+10819), p50 is now 2268 lamports (+771). Attempt 2. To compete in the next Jito leader auction, refresh the expired blockhash, raise the tip to match the current p75 (15000 lamports) to stay competitive in the rising market, then resubmit._
    - attempt 2 [shadow, baseline, 2026-06-19 07:30:05 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 8. `9073c63b8b8a494df148401ee4c172612ec8d51b4eaf0cdc2ceea44d471cb28e`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `2QC7xFoVBPGYt3tx6oY1LZ7WCTSfCPURMZzvsqCnrq3MbXzY4HV5QWeyotSZwkBRBjXX2UkEYqNy7cHMPM3VQxMn`
- **tip_signature:** `4HTcRpD9sYTNAUEGHwUkDUZFQfesMX3D5uiNK6kxGFwMA5qfTemmzavomJCHMimKvSwbaignynHj1fSTLPgLj4GT`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `AtrYi2PV6jYJHBAL5GENoKwUjKwSB36nLDDLfdofze6n`  (fetched at slot 427460623)
- **target / submitted slot:** 427460623  (Jito leader; identity/is_bam not persisted)
- **tip:** 2542 lamports  (market p50 1000, p75 2542 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:29:03 UTC  (slot 427460623)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:30:09 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 2542 lamports (p50 1000, p75 2542 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7372ms, 2026-06-19 07:30:16 UTC] → **refresh_blockhash, set_tip(15000), resubmit**
        - _AuctionLost (Ambiguous confidence): bundle submitted at slot 427460623 with tip 2542 lamports (exactly p75 at submit: 1000/2542) never landed by slot 427460784 (161 slots elapsed, past the ~150-slot blockhash validity window). The blockhash is now stale (161 slots old) as a downstream symptom of sitting unlanded while losing the auction. The tip market has risen sharply: p50 increased 771 lamports to 2268, p75 surged 10819 lamports to 15000 (trend: rising, data age 26 seconds). The original tip of 2542 lamports is now well below the current p75 of 15000 lamports. This is attempt 2. Actions: (1) refresh_blockhash to obtain a valid blockhash for the next 150 slots, (2) set_tip to 15000 lamports to match the current p75 and compete in the rising auction, (3) resubmit to enter the next Jito leader auction with competitive parameters._
    - attempt 2 [shadow, baseline, 2026-06-19 07:30:16 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 9. `a187896edf073537d8aac29df32ab0010e6dc904d590464db5ee000c01f99030`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `39CrChoaVBk7HQGFG4D1oTRVjfNRHPNSVCqW7qim7zDguj5Q1RiJhmyNvj6pXEScELXkzhqFAQMZST6DZaDxZXoo`
- **tip_signature:** `RpwMBUUNmn2UodaQt1UsuzBsW4Sgwvymn2KjZhcmvNoVWox4mDANs9doRgokGrtqdseC31mtJ9qaCFF2vsGbgBC`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `J7veM4mZe8NVmVsyAQiydyHvd34gHPVtXTpqsL3qHCLv`  (fetched at slot 427460655)
- **target / submitted slot:** 427460655  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1000, p75 2542 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:29:17 UTC  (slot 427460655)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:30:25 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1000, p75 2542 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 168 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 4809ms, 2026-06-19 07:30:30 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427460655 with tip 15000 lamports (well above p50 1000 and p75 2542 at submit) never landed; blockhash now aged 168 slots (past ~150-slot validity window) — a downstream symptom of sitting unlanded, not the original cause. Tip 15000 lamports remains competitive (current p50 1663, p75 7259, trend falling). Attempt 2. Refresh blockhash to restore validity, then resubmit with unchanged tip to compete for next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:30:30 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 10. `b365d8ed830bf291a1a64a6765e6c98cb92cdfcc3a23ecd0eaa9343177ca86d0`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `32x2E2CVDhqtiDkNMuicXiJfCBhLLjWdXEuQbqkTXgshFNY1kCMtNGYpug6hBnMo7rKhhEPZUPmy3eudmvsrTSbh`
- **tip_signature:** `2GbcJFd6Gu6DRhZYqygyat7vX7v7xy94x4NMzLJFevqRrobNhWWECMBFdAu7xU7Hrje7MNaN1bQHJb78xxbZmSmt`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `FTjGYtJ7MRbYEqxUKf3p9C7HND2Zig4eKHUNAFXsWbXN`  (fetched at slot 427460682)
- **target / submitted slot:** 427460682  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1497, p75 4181 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:29:27 UTC  (slot 427460682)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:30:33 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1497, p75 4181 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 162 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 4767ms, 2026-06-19 07:30:38 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427460682 with tip 15000 lamports (well above p50 1497 and p75 4181 at submit) never landed; blockhash now aged 162 slots (past ~150 validity window) — a downstream symptom of sitting unlanded, not the root cause. Tip 15000 lamports remains competitive (current p50 1663, p75 7259, trend falling). Attempt 2. Refresh blockhash to restore validity, then resubmit with unchanged tip to compete for next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:30:38 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 11. `896837ee278c99df27da39f2ea52ea18bf7e7e80f4b321d6acbf471a39121794`  (Failed · AuctionLost · Certain)

- **memo_signature:** `2kYwdi5JM7no4CJknkHUGomZTvaHBcLvHNzzHzx8Kv8m8wMLKXSeyJWxzjQKM66xkug7WyUg9KwbVMsEo3sWEncf`
- **tip_signature:** `3DX5y17whF6xPU6ajNNuPFfCGevWhahffuzi3LVv9KdwfggJ1ReThM3jDSqsc8etAJZVWyZpbaJNWCtXHGveSJCF`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `7uqWCd11YVXuYLAk3Mu2mHg7cTEkxvjdh7ASqqaQQ5Bg`  (fetched at slot 427460742)
- **target / submitted slot:** 427460742  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 2268, p75 15000 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:29:52 UTC  (slot 427460742)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:30:57 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 15000 lamports (p50 2268, p75 15000 at submit) (its blockhash later aged to 162 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7006ms, 2026-06-19 07:31:04 UTC] → **refresh_blockhash, set_tip(17000), resubmit**
        - _AuctionLost (Certain): bundle_id was returned but getInflightBundleStatuses=Invalid, the bundle lost its auction. Blockhash age is 162 slots (past the ~150-slot validity window) — a downstream symptom of sitting unlanded, not the root cause. Tip was 15000 lamports, exactly at p75 (15000) when submitted but the market has shifted: p50 rose 5030 lamports to 6693, p75 rose 4156 lamports to 11415, trend is rising. Current tip 15000 is now above p75 (11415) but given the rising trend and prior loss at p75, raising to 17000 lamports (~49% above current p75) improves competitiveness. Attempt 2, so refresh_blockhash to restore validity, set_tip to 17000 to beat the rising auction, then resubmit._
    - attempt 2 [shadow, baseline, 2026-06-19 07:31:04 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 12. `5b34278ef0b718eadd8785d7b2da00e4bcf523bb05809337f500a4393ebce5c7`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4TeokriYyv662iGV6obgZroHTqnpHCkKBEdsZF92bwHCWbwNqMpa37ZGEBASaXRLDifMMQTdiwxd5ZMv6nupwzVF`
- **tip_signature:** `2THwYGKPdPVCdU7rWojkfnLk7cn2W4hJHFBN1JLybbRB2SQLrYEZxS985reS3emMDeTYYUH8q4gsNHd9Wq4twzTc`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `AdT3su2vFLcLQjFXeif5XPtBGnrUxtZHbC9uRa6c8ihz`  (fetched at slot 427460773)
- **target / submitted slot:** 427460773  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 2268, p75 15000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:30:05 UTC  (slot 427460773)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:31:09 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 2268, p75 15000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7104ms, 2026-06-19 07:31:16 UTC] → **refresh_blockhash, set_tip(17000), resubmit**
        - _AuctionLost (Ambiguous confidence): bundle submitted at slot 427460773 with tip 15000 lamports (equal to p75 15000 at submit), never landed, blockhash now aged 161 slots (past ~150 validity window). Tip percentiles have risen: p50 from 2268→6693 (+5030), p75 from 15000→11415 (-4156 but still elevated). The bundle lost its auction despite competitive tip at submit; blockhash expiry is a downstream symptom of sitting unlanded, not the root cause. Attempt 2 of retry sequence. Actions: (1) refresh_blockhash because current blockhash at 161 slots is stale and invalid; (2) set_tip to 17000 lamports (~49% above current p75 11415, ~154% above current p50 6693) to improve auction competitiveness in rising-tip environment; (3) resubmit to compete for next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:31:16 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 13. `4f76ca55c6241eaad79e07e4342fe6e4bdae398ce0e6c948399765cd27b06170`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `23RSFaXu3ioJrqQJBsw4yVmH5Uc5rjMiyANJ849FCjAFMKdDE9x5LJZvUTLvk4E4yQvVCy9DjtsXTGRdK1X3cqJJ`
- **tip_signature:** `2W7dTiUjedMJf61ETGXZTpo6e9XZbEiQYyVTsGkCKM8aZWcfaRfguDT27Fs12g77FMR5aD9qBjMQRYvHMhaayAZK`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `5qRLw4Z1YjWfJsbjoWGgUoxUmG5Dr9nv3dytRYzF6EWC`  (fetched at slot 427460803)
- **target / submitted slot:** 427460803  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 2268, p75 15000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:30:16 UTC  (slot 427460803)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:31:21 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 2268, p75 15000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 163 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7432ms, 2026-06-19 07:31:28 UTC] → **refresh_blockhash, set_tip(20000), resubmit**
        - _AuctionLost (Ambiguous confidence): bundle submitted at slot 427460803 with tip 15000 lamports (exactly p75 at submit: 2268 p50, 15000 p75) never landed; blockhash now aged 163 slots (past ~150 validity window) — a downstream symptom of sitting unlanded, not the root cause. The bundle lost its auction despite tipping at p75, suggesting fierce competition or skipped Jito leader slots. Current percentiles (2096 p50, 10000 p75) have fallen but are stale indicators; the original submit-time p75=15000 shows the real competitive floor. Attempt 2: refresh the expired blockhash, raise tip to 20000 lamports (33% above the original p75) to improve auction odds, then resubmit._
    - attempt 2 [shadow, baseline, 2026-06-19 07:31:28 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 14. `00082a5e7649c5ea12f1f2dd609a23e402b2dd6b3c736b340db18ab03a42df74`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `2ZK7g2DLXAgwqLwUJgkvsY5BND2fUSm8WsrVPhR7tZUnxPfaBYNrLitEohwh2qkwhS3tNtDdv57Eo9KDd7kwTpGt`
- **tip_signature:** `4CK4eJkNknsesS4ers9bT3KDnE3SWWSgXmEhUApAkuuqrcrCjtEFy3KyW5qCcKJutZHV9sHbB2CWvAcagcNMuoSk`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `Hztsc6zXZLBjSvYj9rRHtiWtbrieuxsMJr5PZ2pZCSmn`  (fetched at slot 427460835)
- **target / submitted slot:** 427460835  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1663, p75 7259 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:30:30 UTC  (slot 427460835)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:31:33 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1663, p75 7259 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6709ms, 2026-06-19 07:31:40 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed despite competitive tip of 15000 lamports (p50 1663, p75 7259 at submit). Blockhash age is now 161 slots (427460996 - 427460835), exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the original cause. The tip was well above both percentiles at submission (9x p50, 2x p75) and remains above current percentiles (p50 2096, p75 10000), so the auction loss was not due to an uncompetitive tip. Attempt 2 of this bundle. Correct action: refresh_blockhash to obtain a valid blockhash, then resubmit unchanged to compete in the next Jito leader slot. No tip increase needed as 15000 lamports remains competitive (7.2x current p50, 1.5x current p75)._
    - attempt 2 [shadow, baseline, 2026-06-19 07:31:40 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 15. `ed1fe49f4c55bfe3467031edf71d133aa588272ad76ce348190e2bdc875d0b14`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4FCeAurV2f6TpxHqSGBd9rkekCRqTMVpVKLbzqQM6g8hN4u43PT3Cjp9r9aBosvFA61dGZoLr6tkjikJTKwAHGon`
- **tip_signature:** `3ajfvRxqwLzUazFgdfTspUYRNy8gxZMiuQbxNJXnMMcLFC5pFH8uvU612LGc633Ct2hhD2tWpzEbvSxzdMt4NLcF`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `J5g4XrWNsdsDVKdmxgLBP51Rf6grHKBwWZuPhEUJkWpp`  (fetched at slot 427460856)
- **target / submitted slot:** 427460856  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1663, p75 7259 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:30:38 UTC  (slot 427460856)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:31:45 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1663, p75 7259 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 170 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7826ms, 2026-06-19 07:31:53 UTC] → **refresh_blockhash, set_tip(20000), resubmit**
        - _AuctionLost (confidence Ambiguous with alternative BundleFailure). Blockhash age is 170 slots (427461026 - 427460856), exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The bundle was submitted with tip 15000 lamports when p50 was 1663 and p75 was 7259, well above both percentiles, indicating the tip was competitive at submission. However, the tip environment has shifted: p50 rose from 1663 to 3258 (+1162 lamports, rising trend), and p75 remains at 10000 (was 7259 at submit, implying current data shows 10000). The bundle never landed despite a competitive tip, suggesting it lost the auction or encountered skipped Jito leader slots. On attempt 2, the correct actions are: (1) refresh_blockhash because the blockhash is now stale at 170 slots, (2) set_tip to 20000 lamports to stay comfortably above the rising p75 of 10000 and improve auction competitiveness, (3) resubmit to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:31:53 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 16. `0a1cae53fe596364b15ead3322c9ef17e160af3b32a9180cd1d83cf094fbaffa`  (Failed · AuctionLost · Certain)

- **memo_signature:** `pmb8bMcwiW9D7PKEftJP4pryxVZxoQnqyoTHoFfeUaB8Akf4ScPMCHZ2yA9MHgNQpLW2WtBEkzok9iY7CzUh6Ta`
- **tip_signature:** `29jU3TZgejFA3nD3jZRArjmBWA9UZXn8XxXUZKGxwxkGa4cxBRHHm4Xp6x4vAgSZ8t21rcgkAp1inGoxqbzppBAn`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `DVSpw7bYRGvs35zkh7ghtTuAhRFpmaJ2av5R9WNngE53`  (fetched at slot 427460894)
- **target / submitted slot:** 427460894  (Jito leader; identity/is_bam not persisted)
- **tip:** 11415 lamports  (market p50 6693, p75 11415 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:30:53 UTC  (slot 427460894)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:31:57 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 11415 lamports (p50 6693, p75 11415 at submit) (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 5786ms, 2026-06-19 07:32:03 UTC] → **refresh_blockhash, set_tip(12000), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid—the bundle never won its auction. The blockhash aged to 161 slots (past the ~150-slot validity window) as a downstream symptom of sitting unlanded, not the root cause. The tip was 11415 lamports, exactly at p75 (11415) at submit but not above it—insufficient to win. Current p75 is 10000 lamports with p50 rising (+1162 lamports). Attempt 2. Actions: refresh the stale blockhash, raise tip to 12000 lamports (above current p75 to be competitive), and resubmit to compete in the next Jito leader auction._
    - attempt 2 [shadow, baseline, 2026-06-19 07:32:03 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 17. `a3626b363a8111cf4fbeaed447e192841eee717889a7a7154e8879f6118dd77a`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4x4kJ3rin9dbn667WLy9e7pQ9W8H7dFrQLzLd2CTpgQYeNZKH2AsBFGPJ1kJ51wuGg5oZ8NvFEdQHc1fZTGodG3W`
- **tip_signature:** `5Vt1PWJeug8yJZf5u9ZnMZYNz21CtWk9DW3NdUfsH3jjECxXxSaWxypxaiGggSZCDQKQ4Aini2qgL2hjnWHJVhAo`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `GHBVqiPuCSEmdYuR8qpy6a5P9gqdjEyRH1BAEcHnrEmE`  (fetched at slot 427460922)
- **target / submitted slot:** 427460922  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 6693, p75 11415 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:31:04 UTC  (slot 427460922)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:32:09 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 17000 lamports (p50 6693, p75 11415 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 163 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7173ms, 2026-06-19 07:32:16 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence with BundleFailure alternative): bundle never landed after submission at slot 427460922, now at slot 427461085 (163 slots elapsed). Blockhash age is 163 slots, exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The tip was 17000 lamports, well above p50 (6693) and p75 (11415) at submission, indicating competitive bidding. Current percentiles are p50 3258, p75 10000 (27 seconds old data, p50 rising by 1162 lamports). This is attempt 2. The bundle lost its auction despite a strong tip; the blockhash has now aged past validity. Refresh the blockhash to make the transaction valid again, then resubmit without raising the tip since 17000 lamports remains competitive against current p75 of 10000._
    - attempt 2 [shadow, baseline, 2026-06-19 07:32:16 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 18. `9aaf9d3b84d579a8bdcd3bbcdf8c5e8945fc194c89d1cb393af40393eea05aa7`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `h1vC2Cpv2uShSZCWEQkE699TcPuuTPVGZySvEa83nj7wMNWjfzw72N1byJFnRfKEgHveLiWPVaiFewdr8r3J5DT`
- **tip_signature:** `2JoTMnr8AviHH2FPjhw5CTnuFoa5qWNM937Uef48PaextpwNnvocqAvGC5Z69SwtNp4SsAVQQUvpiQpDBaayoiMi`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `RXNTe9NYXUSgyLXadhyM43uphR4mACHcQyDgsjp9yXY`  (fetched at slot 427460953)
- **target / submitted slot:** 427460953  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 6693, p75 11415 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:31:16 UTC  (slot 427460953)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:32:21 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 17000 lamports (p50 6693, p75 11415 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 163 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 4383ms, 2026-06-19 07:32:25 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence): bundle never landed despite competitive tip of 17000 lamports (well above p50 6693 and p75 11415 at submit). Blockhash aged to 163 slots (past ~150 validity window) as a downstream symptom of sitting unlanded, not the root cause. Tip remains competitive (current p50 2905, p75 10000, trend flat/falling). Attempt 2. Refresh blockhash to restore validity, then resubmit with unchanged tip to compete for next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:32:25 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 19. `b690c8b71a77e2ae2b19d32361ebcf157d8103f2c213ef7031e711c1ae4b8552`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `3vRX3amgvchTe6RoW1FjSmERsJxnevu4MZNrVMDLoWGyJmdAD6ryX8R44e7Cmf9dAgYLYNdx8LYrqZgbVdUcqwuJ`
- **tip_signature:** `97ivFZZePUaSJfUgdpiEXv5SGD4hLsKG4vCDhet3RieCxxs62rvsB6mDyj1KM2NJRPQqvZmswrcN4dMp8sxCF79`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `CrUWfGpwTYT1mb66maNrTEo6JASLbDzJajp2WhWtKjtK`  (fetched at slot 427460984)
- **target / submitted slot:** 427460984  (Jito leader; identity/is_bam not persisted)
- **tip:** 20000 lamports  (market p50 2096, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:31:28 UTC  (slot 427460984)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:32:37 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 20000 lamports (p50 2096, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 172 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7428ms, 2026-06-19 07:32:44 UTC] → **refresh_blockhash, set_tip(25000), resubmit**
        - _AuctionLost (Ambiguous confidence with BundleFailure alternative): bundle submitted at slot 427460984 with blockhash fetched at same slot, never landed, now at slot 427461156 — blockhash age 172 slots exceeds the ~150-slot validity window (downstream symptom of sitting unlanded). Original tip 20000 lamports was 2x p75 (10000) and 9.5x p50 (2096) at submit, well above competitive thresholds, suggesting auction loss or skipped Jito leader rather than uncompetitive tip. Current percentiles: p50 2905, p75 10000 (p50 down 353 lamports, p75 flat, trend not rising). Attempt 2. Actions: refresh_blockhash (blockhash expired after aging unlanded), set_tip to 25000 lamports (raise 25% above original to improve auction odds while remaining well above current p75), resubmit to compete in next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:32:44 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 20. `07493433a3f01cb3e0315e1450a7c589c64f3378c7afb6ee9c0b1fab4366de73`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4kGuuCdk6gS7BpzBsJvwqNTFakrRwQEaqnRKEVtxLNyCVbNiSKbSrqDNmhSEBJKBJMP21aD8fbcx9uxxy5m44wZ`
- **tip_signature:** `376z9x625soTiBae9ECs6T277Wv6Lq4WUcbNdbEu4RT828kUDLCfreH24SwxDGkikzR7vw2gtWuS7jkk5gJinX4n`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `9JL3DVdmhy9wCW467SXKeeuiZGNPm7rEWQG5S7mSmHhr`  (fetched at slot 427461013)
- **target / submitted slot:** 427461013  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 2096, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:31:40 UTC  (slot 427461013)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:32:45 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 2096, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 162 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 9751ms, 2026-06-19 07:32:54 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed after submission at slot 427461013, now at slot 427461175 (162 slots elapsed, exceeding the ~150-slot validity window). Tip was 15000 lamports, well above p50 2096 and p75 10000 at submit (and still above current p50 2062, p75 10000), so the tip was competitive. The blockhash is now stale (162 slots old) as a downstream symptom of the bundle sitting unlanded while losing its auction or encountering skipped Jito leader slots. This is attempt 2. Refresh the blockhash to make the bundle valid again, then resubmit without raising the tip since 15000 lamports remains competitive (p75 unchanged at 10000, p50 declining)._
    - attempt 2 [shadow, baseline, 2026-06-19 07:32:54 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 21. `95ef743b604c924325d3acbbf51015402df17627b04e2254067ae361e59065cd`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `2jdPNNzwFNqD92pnn5mFfGuGjePMKufNq6JsoxAXiJpctdR1RaHTveCxtg6ZLerzjWV5vNpzqxx5AUcvxahQpmde`
- **tip_signature:** `2DKYQqHSwXJ5XB54CcU6ye7QZ2xLhxUsifZZijtocS9jx4FfztHB9UguDsx7wT7dDqqWxPB1497dAeMytY2Ye6pF`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `44d7DpDVQebDMMAKgYwGYaSZRU7JV7uEMDKDoUTCxWrg`  (fetched at slot 427461045)
- **target / submitted slot:** 427461045  (Jito leader; identity/is_bam not persisted)
- **tip:** 20000 lamports  (market p50 3258, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:31:53 UTC  (slot 427461045)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:32:57 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 20000 lamports (p50 3258, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 162 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7973ms, 2026-06-19 07:33:13 UTC] → **refresh_blockhash, set_tip(25000), resubmit**
        - _AuctionLost (Ambiguous confidence with BundleFailure alternative): bundle submitted at slot 427461045 with tip 20000 lamports (2x p50 3258, 2x p75 10000 at submit) never landed; blockhash now aged 180 slots (past ~150 validity window) — a downstream symptom of sitting unlanded, not the root cause. Tip was competitive at submission but bundle lost its auction or encountered skipped Jito leader slots. Current tip percentiles: p50 2062, p75 10000 (p50 fell 843 lamports, p75 unchanged, not rising). Attempt 2. Actions: (1) refresh_blockhash because at 180 slots the blockhash is expired and must be rebuilt; (2) set_tip to 25000 lamports (2.5x current p75) to increase competitiveness for the next auction given the prior loss despite a seemingly adequate tip; (3) resubmit to compete for the next Jito leader slot with the fresh blockhash and higher tip._
    - attempt 2 [shadow, baseline, 2026-06-19 07:33:13 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 22. `abc80a66d5ebed021cb10e8c9c7960dde5554ea502b7e07ad60fd763b8566375`  (Failed · AuctionLost · Certain)

- **memo_signature:** `ZDuPyMxTJxRU4gLBXYd5dLKk1owyWLR4mmWC5CLFsr5X2fxeYLFijoHp41b6ZThg2nQ68J1ETmbjxxc9y4tP5zK`
- **tip_signature:** `4BNFRtpcKQFPY7kWFjkNx6bAF8gP7LZ1mLkE4qu2PHimRGyb8ipXhQkPSVXd6FKwSMJej1FKvpNnn3UMaJTqipv9`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `44d7DpDVQebDMMAKgYwGYaSZRU7JV7uEMDKDoUTCxWrg`  (fetched at slot 427461046)
- **target / submitted slot:** 427461046  (Jito leader; identity/is_bam not persisted)
- **tip:** 10000 lamports  (market p50 3258, p75 10000 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:31:53 UTC  (slot 427461046)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:32:57 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 10000 lamports (p50 3258, p75 10000 at submit) (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7219ms, 2026-06-19 07:33:04 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid — the bundle never won its auction. Blockhash aged to 161 slots (past ~150 expiry) as a downstream symptom of sitting unlanded, not the root cause. Tip was 10000 lamports, exactly at p75 (10000) at submit time, so it was competitive. Current p75 is still 10000 lamports (p75_change 0 lamports, not rising). Attempt 2. Refresh the blockhash (it is now stale) and resubmit at the same tip to compete for the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:33:04 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 23. `d9365cb4cb3fd5b67e74a84dbc3a765ed342b02481cc618674bd6feb804e1d40`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `hRCqzsUyKzvQchns51GyMGqJATTB9yUsfUbiw3DmpMCFYJcohxVvWHnQ5avnz9yVfkGikbYFnP7FFA71uqqJ7aH`
- **tip_signature:** `4YGjr3yGYQMnfDesoVwAuPEPdDkf31CUbqm3szVMbiivqbFKJY7VEAHZBMTeqaiu8H8qNgQFqaH2ZLT3VugTAK1K`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `Ea9wYzRKysW7s77rwk5YenUJiuH1nRadY6ErWQnjPMYm`  (fetched at slot 427461069)
- **target / submitted slot:** 427461069  (Jito leader; identity/is_bam not persisted)
- **tip:** 12000 lamports  (market p50 3258, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:32:03 UTC  (slot 427461069)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:33:13 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 12000 lamports (p50 3258, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 178 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6569ms, 2026-06-19 07:33:19 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461069 with tip 12000 lamports (above p50 3258, above p75 10000 at submit) never landed; blockhash now aged 178 slots (past ~150 validity window) — a downstream symptom of sitting unlanded, not the root cause. Tip was competitive (20% above p75 at submit; current p75 8106 is lower, trend falling). Attempt 2. The bundle lost its auction or encountered skipped Jito leader slots; the aged blockhash must be refreshed to retry. No tip raise needed (original tip remains well above current p75). Resubmit after refresh to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:33:19 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 24. `03b8565f609d64f2a741eccc555bc15dc03f7a13f01c448142a02194ecc1c871`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `39FtCAzvouNoCCAbUd4HdBptKg22416wJS9gd8PdVydxsw7RF4wDthC222u7BoH2CJcAzFmZ8Tvw8jCQZWoaivEf`
- **tip_signature:** `5BXKdtZeQU8zYLDoLUyUidkAe3c3KoTwuYBEHUD8dCgnfpStz5ho3oiHPi8b4TzMk5JuCmw3KHLQcAMiEvWFupxN`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `FBQzrR5fyjPDhY4J72HRLvJ8iU957dtGum2xSsfPTdfy`  (fetched at slot 427461103)
- **target / submitted slot:** 427461103  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 3258, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:32:16 UTC  (slot 427461103)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:33:21 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 17000 lamports (p50 3258, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7365ms, 2026-06-19 07:33:28 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (confidence Ambiguous with alternative BundleFailure). Blockhash age is 161 slots (427461264 - 427461103), exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The bundle was submitted with tip 17000 lamports against p50 3258 and p75 10000 at submit time, well above both percentiles, so the tip was competitive. Current percentiles are p50 1620 and p75 8106 (both falling: p50 down 442, p75 down 1894), so the original 17000 lamports remains highly competitive. This is attempt 2. The bundle likely lost the auction or encountered a skipped Jito leader slot. The correct actions are: (1) refresh_blockhash because the blockhash is now stale at 161 slots, and (2) resubmit with the existing tip (17000 lamports) which remains well above current percentiles. No tip increase is warranted given the tip was and remains competitive._
    - attempt 2 [shadow, baseline, 2026-06-19 07:33:28 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 25. `4fa85a854eaa67b045272e59c99f0b0d952fd26e9c483f8ee6cafeee0c5af0e8`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4hrHKsk3HqxcXQE8mM5kW5TUYpFuKchEhsMDuBzLHbzMnxac5S9uFUcc1bFMa7VkPnWJCbWpz2Q8yvHajZnX4ZEb`
- **tip_signature:** `61QtRkn67MXiBrDshu9WZ1ud9y32uCUM21k3VSh28FdCNtYKMi8yNeKYQpTsKcru7j6i5wGaTNCudP7V5F2ZaQC1`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `jhwDqHLr9jzLLg5kF8DjiQy562aszwABxJwJayN5spb`  (fetched at slot 427461127)
- **target / submitted slot:** 427461127  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 2905, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:32:25 UTC  (slot 427461127)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:33:33 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 17000 lamports (p50 2905, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 167 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7243ms, 2026-06-19 07:33:40 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461127 with tip 17000 lamports (well above p50 2905 and p75 10000 at submit) never landed; blockhash aged to 167 slots (past ~150 validity window) as a downstream symptom of sitting unlanded, not the root cause. Tip 17000 lamports remains competitive (current p50 1620, p75 8106, trend falling -442/-1894 lamports). Attempt 2. Refresh blockhash (expired at 167 slots) and resubmit with unchanged tip to compete for next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:33:40 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 26. `81054e7cba689c9c50535b9910ed0026d7767c11c391d4361f18df12906efff5`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `5zqUhG2S9ykkTff8MXnNSh5QUdspNWhyZueefNJRew2JzJwumjnQFfw3oGjYFHoN9d9Rd7Licrq1dXr8CjYzwmVZ`
- **tip_signature:** `2XjsMLWfWY4zB82RMC16gCQAsPozfDSQrEqFX366DgMZp6KGL1QX4nVPvvLkVo4CjQ77aMuKbErD5FUWNLKApWaD`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `Hn1CAz41xzzfakG6aecF7UaUVxbyqe7jVT3huwFPNHmR`  (fetched at slot 427461175)
- **target / submitted slot:** 427461175  (Jito leader; identity/is_bam not persisted)
- **tip:** 25000 lamports  (market p50 2905, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:32:44 UTC  (slot 427461175)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:33:53 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 25000 lamports (p50 2905, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 168 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7374ms, 2026-06-19 07:34:00 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461175 with blockhash fetched at same slot, never landed, now at slot 427461343 → blockhash age 168 slots (past ~150-slot validity window, stale). Tip was 25000 lamports, well above p50 2905 and p75 10000 at submit; current p50 1459, p75 2819 show falling trend (-161 p50, -5287 p75). Tip was competitive when submitted; bundle likely lost auction or encountered skipped Jito leader slot, then aged while unlanded. Attempt 2. Refresh blockhash (expired), keep tip unchanged (still far above current percentiles), resubmit to compete for next Jito leader._
    - attempt 2 [shadow, baseline, 2026-06-19 07:34:00 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 27. `864fe86e54cb1b6b4f6e7c113cb0e5af84c7a91186ba0bfd2548ef3e6cc7e8f5`  (Failed · AuctionLost · Certain)

- **memo_signature:** `665pztDY1GQpLjeJ2CkrUUPZagPfeJAf3GUzhfax8gW1EwX1zyTTSnDV4yJXv26Cx9QxufK5qjk9t3dzbp1K32fR`
- **tip_signature:** `3NQteSDYRB3VFH2tqc6s7hntARUwL5gEcsi95KCe7Qxy9hkcGYRgf26JZMGuF1WHvQTMJdsqtPRaCyTzJTMeDa8a`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `3BZgftEMXXQMCdcUBwBCqroQf1mSxhxtWYJqiTfNuVb3`  (fetched at slot 427461198)
- **target / submitted slot:** 427461198  (Jito leader; identity/is_bam not persisted)
- **tip:** 10000 lamports  (market p50 2062, p75 10000 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:32:54 UTC  (slot 427461198)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:34:01 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 10000 lamports (p50 2062, p75 10000 at submit) (its blockhash later aged to 164 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6800ms, 2026-06-19 07:34:15 UTC] → **refresh_blockhash, set_tip(15000), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid — the bundle never won its auction. Blockhash age is 184 slots (past the ~150-slot validity window) — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The tip was 10000 lamports, exactly at p75 (10000) at submit time, but the bundle still lost. Current percentiles have dropped (p50: 1459, p75: 2819, trend falling -5287 lamports at p75), but this may reflect temporary market conditions. On attempt 2, refresh the blockhash (it aged while unlanded) and raise the tip to 15000 lamports (50% above the original p75) to improve competitiveness in the next Jito leader auction, then resubmit._
    - attempt 2 [shadow, baseline, 2026-06-19 07:34:15 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 28. `35647b9243af5b3f69f6a2985233cd1988b1cc5c665076f526274b6ac221d7ac`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4m7T1Bv6wKxj6GzBmoSgGdQACMCmXvpnx44mdqLrjypLoQwbn4y4QNroiTb4Ckw9qjfeZryB29nTPDPCEbut6USQ`
- **tip_signature:** `ynAkTeVpFvuTBRX1se3BZqTi9dH2aGck7XstNvuN59CWRbxscgzdk6CmFtQmZXTGE6qcLDnGGTwB35BW3FpNqnT`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `CCDi21fkR9yxVqQoSQXexXah2vv9c4ejUfzcrwgLgXvv`  (fetched at slot 427461200)
- **target / submitted slot:** 427461200  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 2062, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:32:54 UTC  (slot 427461200)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:34:01 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 2062, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 162 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7003ms, 2026-06-19 07:34:08 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461200 with blockhash fetched at slot 427461200, never landed, now at slot 427461362 → blockhash age 162 slots (past the ~150-slot validity window, a downstream symptom of sitting unlanded). Tip was 15000 lamports at submission (well above p50 2062 and p75 10000 at submit; currently p50 1459, p75 2819, both falling). The tip was competitive; the bundle lost the auction or encountered a skipped Jito leader slot. Attempt 2. Refresh the blockhash (it is now stale) and resubmit with the existing tip (15000 lamports remains far above the current p75 2819 and the falling trend indicates no need to raise it further)._
    - attempt 2 [shadow, baseline, 2026-06-19 07:34:08 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 29. `b3ba02b6a9afb9b5c01bbbcd5ba67aba037241afce993e7d19648c9e5d40e3a8`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `5QN51sraB4Yb4mQHJK1D64piN4b1CzJcL7mGByTYMk31T8EpRvJg6zSsWPF7LafrosPFhNuuqpmMBLyU6UaNCjPv`
- **tip_signature:** `5JsYN14ZpvdmuGuQ9JunnyhCLziBxNoMrb16chp1Ss6uTc7efULjDdSEu1axUb1u1pEd2PjW9f6Q61qvjymcTokS`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `2g9w14msaoEwwNNYM3f17LMX5SBLJZGT21jPGw9K2t4k`  (fetched at slot 427461225)
- **target / submitted slot:** 427461225  (Jito leader; identity/is_bam not persisted)
- **tip:** 10000 lamports  (market p50 2062, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:33:04 UTC  (slot 427461225)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:34:15 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 10000 lamports (p50 2062, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 174 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7281ms, 2026-06-19 07:34:22 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence): bundle never landed after submission at slot 427461225, now at slot 427461399 (174 slots elapsed, exceeding the ~150-slot blockhash validity window). Tip was 10000 lamports, exactly at p75 (10000) at submit time, indicating competitive bidding. Current tip percentiles are p50 1645, p75 4672 (both lower than our 10000 tip), confirming the original tip was strong. The blockhash aged to 174 slots as a downstream symptom of the bundle sitting unlanded, not the root cause. Attempt 2 of this bundle. Correct actions: refresh_blockhash (the blockhash is now stale at 174 slots) and resubmit with the existing 10000 lamport tip (which remains well above current p75 of 4672). No tip increase needed given our tip is 2.14x the current p75._
    - attempt 2 [shadow, baseline, 2026-06-19 07:34:22 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 30. `3c1a94748d722f6ea66e38abe0d80b91fb7729c0150923e111cd7d93c3348137`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `2t2ESctKmdfKiqf2mMMv9s3XdDpBHfbDvdoPQZdw2UWQLJo62xQdVPZixRjEauFcWNzADmLqJGAH7yYytxNBPmid`
- **tip_signature:** `4XUESjLvmHJjR2vzAAwMvxwhmK4nTnH7VSQaXiCkHafNZwS7qRMyhzAT6CmF42YtQqBrgs4G6shvmhujusEeCfqk`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `2xf3tQ5dyzQkUgJdpfzTxUHdUM4gKdqgWH6Vckg169px`  (fetched at slot 427461246)
- **target / submitted slot:** 427461246  (Jito leader; identity/is_bam not persisted)
- **tip:** 25000 lamports  (market p50 2062, p75 10000 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:33:13 UTC  (slot 427461246)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:34:22 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 25000 lamports (p50 2062, p75 10000 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 172 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7659ms, 2026-06-19 07:34:30 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461246 with blockhash fetched at same slot, never landed, now at slot 427461418 — blockhash age 172 slots exceeds the ~150-slot validity window (downstream symptom of sitting unlanded). Tip was 25000 lamports, well above p50 2062 and p75 10000 at submit; current p50 1645 and p75 4672 show tip remains highly competitive (10x+ above p75). Attempt 2. The bundle lost its auction or encountered skipped Jito leader slots while the blockhash aged out. Refresh blockhash to restore validity, then resubmit with unchanged tip (already far above prevailing percentiles). No tip raise needed — 25000 lamports dominates the current p75 of 4672._
    - attempt 2 [shadow, baseline, 2026-06-19 07:34:30 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 31. `bb22dbfa9169c3cfb3e852a5ade201a158b77c1c849f4508d01af8d0447e904d`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4vnHTgpgRdCYogwrkJWsaaF3aSCKctL4rzgycaNXfNJxfK4GMFYUjT7LyxjCi778o1NMTm1b14BX5o1d1keDmdLd`
- **tip_signature:** `BKqhJbkzJDmyhD6t9vHkZTga7nfKDGsR68E5rqW5jdZq16XFxNHymfTM8Mo8xks4Rzgm5RELuGGDFVpRPmXPwzz`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `CmKPLaYb5xDK9kFzfxNNfHCcnfZrwFC81czWSULtEZ7f`  (fetched at slot 427461261)
- **target / submitted slot:** 427461261  (Jito leader; identity/is_bam not persisted)
- **tip:** 12000 lamports  (market p50 1620, p75 8106 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:33:19 UTC  (slot 427461261)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:34:30 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 12000 lamports (p50 1620, p75 8106 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 177 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6979ms, 2026-06-19 07:34:37 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed after submission at slot 427461261, now at slot 427461438. Blockhash age is 177 slots, exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The tip was 12000 lamports, well above p50 (1620) and p75 (8106) at submission, and remains above current p50 (1645) and p75 (4672), so the tip was competitive. Attempt 2 of this bundle. The bundle lost its auction or encountered a skipped Jito leader slot. The blockhash is now stale (177 slots), so refresh_blockhash is required. The tip remains competitive (12000 >> 4672 p75), so no tip increase is needed. Resubmit with the fresh blockhash to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:34:37 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 32. `f0a7e4edfb2c42d3fee0487be3be04e139db5383dc40bb39d7a691121546f7bf`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4tKQUgtA36NGtxUw6wbsjSZBbXTfYhz2bBUSXfpQMj5BD1eT2nYzHJN6JbKxUPXaptWhLNozyKcMs1J7VRTFaRiG`
- **tip_signature:** `3m3uCzCRq4oYfu7Zgt1brX9CWyqvoaBpfJmwMEtCUkbokGjpnwEduEQVdaB1jAGYkvP53chBJHU3C3s3pVVQcbBQ`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `GQ1XQfhVUMjmQPi5ztASUTCHSBpo8JAGhTPrd4cfmnaF`  (fetched at slot 427461282)
- **target / submitted slot:** 427461282  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 1620, p75 8106 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:33:28 UTC  (slot 427461282)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:34:38 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 17000 lamports (p50 1620, p75 8106 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 175 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 5764ms, 2026-06-19 07:34:43 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461282 with tip 17000 lamports (well above p50 1620 and p75 8106 at submit) never landed; blockhash now aged 175 slots (past ~150-slot validity window) — a downstream symptom of sitting unlanded, not the root cause. Tip 17000 lamports remains highly competitive versus current p50 1645 and p75 4672 (p50 rose only 186 lamports, p75 rose 1853 lamports). Attempt 2. The bundle likely lost the auction or encountered a skipped Jito leader slot; the blockhash must be refreshed because it is now stale (175 > 150), then resubmit with the same tip since it was and remains well above prevailing percentiles._
    - attempt 2 [shadow, baseline, 2026-06-19 07:34:43 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 33. `2f8fcd7f24912c1bfdabdb6fe8260729b0460ec5f5b1c9e153d6261c43bc4068`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `wHizyaMJDyS4RyCXhxX8inR1xiVZZkUS5CtK8Y1xWFiVi7aqoLJYS4RMRhEudwGTBqzfqzfk9cQzUSv44mRxGgw`
- **tip_signature:** `qbcGeDiR4DJqQ2y6XWskWsBKepxyPFkrbXeBU8qEPUGi5pUQ8qCuPynKptqof2Pkd1P3sMbQ3azCpXUUBFTUjVt`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `9Ev3g4BCTyYvCHhPEn5cMXGsfYkoYohNpLJgHBCi9t6k`  (fetched at slot 427461312)
- **target / submitted slot:** 427461312  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 1620, p75 8106 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:33:40 UTC  (slot 427461312)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:34:45 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 17000 lamports (p50 1620, p75 8106 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 163 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 4834ms, 2026-06-19 07:34:50 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence): bundle never landed after submission at slot 427461312, now at slot 427461475 (163 slots elapsed, exceeding the ~150-slot blockhash validity window). Tip was 17000 lamports, well above p50 (1620) and p75 (8106) at submit, and remains above current p50 (2462) and p75 (8590), so the tip was competitive. The blockhash aged to 163 slots as a downstream symptom of the bundle sitting unlanded, not the root cause. This is attempt 2. Refresh the blockhash to restore validity, then resubmit with the existing tip (17000 lamports) which remains competitive._
    - attempt 2 [shadow, baseline, 2026-06-19 07:34:50 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 34. `a965b842f91c014de4a589b75c64e4bb46a0b84738de017fc3bf49b7858bd323`  (Failed · AuctionLost · Certain)

- **memo_signature:** `2tVKzviRPHcFgqFMiiB1wPFgr7iW1npcx7MKJTRa6DXYdX6kTKKwjJaMTV6HmgsPjVmYz8xo3oa2bdNW1n3hiWjF`
- **tip_signature:** `vwj9fePmMckYkEKW7An8NvPWEdQsHQBTV9ukFRx4mAksuwvcnd4uC35qpiMXrC355jdRRVF2th3XETiVYFK8Z8u`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `8irCrZvR5rxBnYnaWgmUYjHEtXXYLNspnQchhoupW3vS`  (fetched at slot 427461350)
- **target / submitted slot:** 427461350  (Jito leader; identity/is_bam not persisted)
- **tip:** 2819 lamports  (market p50 1459, p75 2819 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:33:56 UTC  (slot 427461350)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:35:01 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 2819 lamports (p50 1459, p75 2819 at submit) (its blockhash later aged to 167 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 5419ms, 2026-06-19 07:35:06 UTC] → **refresh_blockhash, set_tip(8590), resubmit**
        - _AuctionLost (Certain): Block Engine accepted bundle_id but getInflightBundleStatuses returned Invalid—bundle lost auction. Blockhash aged to 167 slots (past ~150 validity window) as downstream symptom of sitting unlanded. Tip was 2819 lamports, exactly at p75 (2819) at submit but market rose: p50 now 2462 (+817), p75 now 8590 (+3918). Attempt 2. Actions: refresh_blockhash (167 slots exceeds validity), set_tip to current p75 8590 lamports (match rising competitive threshold), resubmit to compete in next Jito leader auction._
    - attempt 2 [shadow, baseline, 2026-06-19 07:35:06 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 35. `c615e807d59a0c729f6aba588dea07a1f4aaf9994239e6c95744c7da51b916f5`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `5TVEC9ckc1myYrxgsq2cQcYbKqcfesB2kNVoJhKjREs1by8NuxQ41Tg4kyE7CxJHn5Ymc9vGULwVCHzr5uucgBpz`
- **tip_signature:** `3UrpsWaPPg9ZAQhX22hEaSScUGy1V8DdwFeKfc2Nr33qrBeTQySxyCGYiPA9KQPUvAvY6duN4g44HGhHguT8L8gL`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `EWQ57m9TAMRapWymDtfLCUXJ52a4XAnXc2qx69aDjJR`  (fetched at slot 427461362)
- **target / submitted slot:** 427461362  (Jito leader; identity/is_bam not persisted)
- **tip:** 25000 lamports  (market p50 1459, p75 2819 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:34:00 UTC  (slot 427461362)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:35:07 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 25000 lamports (p50 1459, p75 2819 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 169 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [EXECUTED BASELINE FALLBACK (LLM call failed), baseline, 2026-06-19 07:35:17 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 36. `4b6172c36959cb126c95e129a25418959a8973013b5febd8a886188ea7caa938`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `2VvhPZtvJvRFv28zUofoiYULHoR8ewdzMWjtNHMh2vZNT6mxgagy4bBhQVh3LKAaVd8vjRCpCa2aECTneG2bTfMJ`
- **tip_signature:** `225UzBJ9wS4Pg6TQWohsyYNnBWfxcQRnbvF9gCyqarnKn9bJEXFoPdV2fUzgr7QtBtSSSMvQTEEqaiTGfxq8R8Eo`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `HEN6jCr77RW1GHZMvUuHzKpqLKK3EriHzBUMMUseSrDN`  (fetched at slot 427461381)
- **target / submitted slot:** 427461381  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1459, p75 2819 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:34:08 UTC  (slot 427461381)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:35:17 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1459, p75 2819 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 175 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 8336ms, 2026-06-19 07:35:25 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461381 with blockhash fetched at same slot, never landed, now at slot 427461556 → blockhash age 175 slots (past ~150-slot validity window, stale). Tip was 15000 lamports, well above p50 1459 and p75 2819 at submit (10.3× p50, 5.3× p75), and remains above current p50 1666 and p75 4478 (9.0× p50, 3.4× p75) — tip is competitive, no raise needed. Attempt 2. Root cause: lost auction or skipped Jito leader slot (bundle never won inclusion while blockhash was valid); blockhash expiry is a downstream symptom of sitting unlanded, not the original cause. Correct actions: refresh_blockhash (blockhash now stale at 175 slots), then resubmit unchanged (tip remains competitive, no infrastructure failure indicated)._
    - attempt 2 [shadow, baseline, 2026-06-19 07:35:25 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 37. `8eb0420c2ba9f77f83a589a39f8bd89759d408c0996da8b417bfe80aff4e414d`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `3JRvYRoR5mG6M2Jcw5eLDxDdoJ516d6f6NPFoUTtU3vrUyg42Z5mzG55SHaMgz4cdtmNaBqJQnsfkgWEhAh8UqBS`
- **tip_signature:** `RTZCykFAch9WEkbsX8LtmzdRNzLuzWd9EZjZF3UjTNQgbPudKEm4h2y4kKdrTrMiR7LMwYfXKQRxzR2H6Rke5k1`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `9GRESiEVLxz6pneE713yMy1qWDfp7qvCknBFaowj5Cxn`  (fetched at slot 427461398)
- **target / submitted slot:** 427461398  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1459, p75 2819 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:34:15 UTC  (slot 427461398)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:35:26 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1459, p75 2819 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 180 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7573ms, 2026-06-19 07:35:40 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (confidence Ambiguous with alternative BundleFailure). Blockhash age is 196 slots, exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The bundle was submitted at slot 427461398 with tip 15000 lamports, well above p50 1459 and p75 2819 at submit time. Current percentiles are p50 1666 and p75 4478 (tip data age 19 seconds). The tip of 15000 lamports remains 9.0x current p50 and 3.4x current p75, so the tip is still highly competitive. This is attempt 2. The bundle lost its auction despite a strong tip, likely due to competition or a skipped Jito leader slot. The blockhash must be refreshed because it is now stale (196 > 150 slots). No tip increase is needed because 15000 lamports is already far above the prevailing percentiles. Resubmit with the fresh blockhash to compete in the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:35:40 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 38. `f6d46b0ccdb46bdd39c11ca4f1c1d8c43e9ea2513228129b597a62bdbf1405fd`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4T69yzoPRH6mcKMxSGgA75FrunissyEqooYdCeL2gKQPKJHHa4djvq5pXYrptkqdjhJjXHzkC1rSU5PuWpfLbgTj`
- **tip_signature:** `2wSG4UHDDb8tfS9awhNwSeAyRzu5HEsPFSaNkAy4efjcfoRJT2XqeA85cfTtGgqhS3isk5xamFABv4RoHobMrgyw`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `BjjsZJ3NCKqMKL5t4hWRULHBfS3ycycai8WVjgweoM96`  (fetched at slot 427461417)
- **target / submitted slot:** 427461417  (Jito leader; identity/is_bam not persisted)
- **tip:** 10000 lamports  (market p50 1645, p75 4672 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:34:22 UTC  (slot 427461417)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:35:26 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 10000 lamports (p50 1645, p75 4672 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 161 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6151ms, 2026-06-19 07:35:32 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed after submission at slot 427461417, now at slot 427461578 (161 slots elapsed, exceeding the ~150-slot blockhash validity window). Tip was 10000 lamports, well above p50 1645 and p75 4672 at submit (currently p50 1666, p75 4478, trending down -796/-4112 lamports). The tip was competitive; the bundle likely lost the auction or encountered skipped Jito leader slots. Blockhash is now stale (161 slots old) as a downstream symptom of sitting unlanded. Attempt 2. Action: refresh_blockhash to obtain a valid blockhash, then resubmit with the existing tip (10000 lamports remains above current p75 4478)._
    - attempt 2 [shadow, baseline, 2026-06-19 07:35:32 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 39. `c8714d582e90723af534dbb1792433d876510cc3f8d1b0626321ab0e7dc27e94`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `5JoNdWCm99nre85ScgSWDq62m4U7iotLestnXCA5bV3JYsXRiGHgV5DMmCfYGZtCicNejfvvJPSasPyTnHnQ2u35`
- **tip_signature:** `G4BVEUBr1JWjcKRhhTPjHXG57JEfKrVhERwJDsQZ7pxeHk4sb9EFEaDuXEgxHADtwCaTFvYjapsXpn3apB8RH5r`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `HUXtWvM6oo6G3ae6PifSQFKRVJboL51z8wp8C6TQtANY`  (fetched at slot 427461438)
- **target / submitted slot:** 427461438  (Jito leader; identity/is_bam not persisted)
- **tip:** 25000 lamports  (market p50 1645, p75 4672 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:34:30 UTC  (slot 427461438)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:35:40 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 25000 lamports (p50 1645, p75 4672 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 174 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7359ms, 2026-06-19 07:35:47 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461438 with blockhash fetched at slot 427461438, never landed, now at slot 427461612 — blockhash age 174 slots exceeds the ~150-slot validity window (downstream symptom of sitting unlanded). Tip was 25000 lamports, well above p50 1645 and p75 4672 at submit (and still above current p50 1666, p75 4478), so tip was competitive. Jito inflight status was NotPolled (no definitive Invalid/Failed confirmation). Attempt 2. Root cause: lost the auction or a skipped Jito leader slot prevented landing. Blockhash is now stale (aged while unlanded). Correct response: refresh_blockhash to obtain a valid blockhash for the next attempt, then resubmit unchanged (tip remains competitive, no need to raise it). Do not abandon — the bundle was well-formed and well-tipped, just unlucky._
    - attempt 2 [shadow, baseline, 2026-06-19 07:35:47 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 40. `1b64ccb843eb156735f4a6a1115005ca142678759084c02b58c4de17c5abb4f9`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `63EE3cGcZEbdus91hH41RzhyNJeLjZ9gPxSRmtF8v617CpLgBWyvU9HvczoqkW5dcfw8fwHjugkURUFPQ6QxbQz9`
- **tip_signature:** `2nhvMzdxUd1BzMKzC2oW1Wf2kZSPnkJNF62N3gxd27qkR4zq7Rrh3DCRgDEDzjorKe2MFvBZiH85svgqF72K3oNz`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `8tSypQzCPdXD2T2qxQFnhAwVgZbFjp6EGH7cGx9Umtyr`  (fetched at slot 427461456)
- **target / submitted slot:** 427461456  (Jito leader; identity/is_bam not persisted)
- **tip:** 12000 lamports  (market p50 1645, p75 4672 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:34:37 UTC  (slot 427461456)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:35:48 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 12000 lamports (p50 1645, p75 4672 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 177 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 9183ms, 2026-06-19 07:35:57 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461456 with blockhash fetched at 427461456, never landed, now at slot 427461633 — blockhash age 177 slots exceeds the ~150-slot validity window. Tip was 12000 lamports, well above p50 1645 and p75 4672 at submit (and still above current p50 1926, p75 5175), so the tip was competitive. The bundle lost its auction or encountered skipped Jito leader slots; the aged blockhash is a downstream symptom of sitting unlanded, not the root cause. Attempt 2. Refresh the blockhash to make the bundle valid again, then resubmit with the existing tip (no raise needed — 12000 lamports remains competitive above current p75 5175)._
    - attempt 2 [shadow, baseline, 2026-06-19 07:35:57 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 41. `80e7fd8352e5651f44bb3f02de2c0ccb19ccab374c9b9624fbfe11494b3824b6`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `47kWBTZAw42QJhPF9ziCshsPQQEKkk3vX5N7ne1mw331oDfwEpxBzwKkfgtMhVrryfna5uDdFaBg5eRYEM1tnDjR`
- **tip_signature:** `xtmUT4A1JLRwwX1AcxTwxxoZrcuxje45DQxNhw8JQqufiYEkhGe7NiV5e7wicmEqSZQSypBXFEETxzh9r6nvo5q`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `cpMWx8fSzTmBhAUqAuHKE91qNJuZWp4YB8PoDt6Xnc1`  (fetched at slot 427461471)
- **target / submitted slot:** 427461471  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 1645, p75 4672 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:34:43 UTC  (slot 427461471)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:35:48 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 17000 lamports (p50 1645, p75 4672 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 162 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 5395ms, 2026-06-19 07:36:03 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle never landed after submission at slot 427461471, now at slot 427461657 (186 slots elapsed, exceeding the ~150-slot blockhash validity window). Tip was 17000 lamports, well above p50 1645 and p75 4672 at submit (and still above current p50 1926 and p75 5175), so the tip was competitive. The blockhash aged to 186 slots as a downstream symptom of the bundle sitting unlanded, not as the root cause. This is attempt 2. Refresh the blockhash to restore validity, then resubmit with the same tip (17000 lamports remains competitive against current percentiles)._
    - attempt 2 [shadow, baseline, 2026-06-19 07:36:03 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 42. `5f34ebd5d33b9d3225e046dbd9266f47d5a9f44d9150ee49a5bff35abd671985`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `34x9GBrFPkdHposUJL1FoxbW46tdJjVbD4PKTUbR2Z8iKXQZgPuiXY7hsdihFDsoB9C18D1jDnzRnFR5ubQx6MU5`
- **tip_signature:** `2EMNjVBnQmufDb1jSD7tQpnZJomb6sQzRq3C5CASshtKFfjDiqNv4g8fDdaBzCrrHNLopmVceASwMZ6s2SD28jju`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `CTAkmHW3MQ9N2XAf57fec4DdeVDUgLz2p9aGAjeY9End`  (fetched at slot 427461488)
- **target / submitted slot:** 427461488  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 2462, p75 8590 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:34:50 UTC  (slot 427461488)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:36:03 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 17000 lamports (p50 2462, p75 8590 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 183 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6533ms, 2026-06-19 07:36:10 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure). Blockhash age is 183 slots, well past the ~150-slot validity window — a downstream symptom of the bundle sitting unlanded, not the original cause. The tip was 17000 lamports at submission (p50 2462, p75 8590), far above both percentiles, so the tip was competitive. Current percentiles are p50 1926, p75 5175 (rising trend +260/+697 lamports), but the original 17000 lamports remains well above both. This is attempt 2. The bundle likely lost the auction or encountered a skipped Jito leader slot. The blockhash must be refreshed because it is now stale (183 > 150), but the tip does not need adjustment — 17000 lamports is still competitive. Resubmit with the fresh blockhash to compete for the next Jito leader._
    - attempt 2 [shadow, baseline, 2026-06-19 07:36:10 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 43. `65d70f8aa254c53c68cc3617e2933872478c2d5d42633c08d5230f5a33ad61f3`  (Failed · AuctionLost · Certain)

- **memo_signature:** `3xjUnquqGh4vygFmwG7rKMFU3ceMpsz63qhgWstL59CkRp5T57TCsBGuG8gcQBqHRJSUcqdy5qk1W1NnGaf7SmvN`
- **tip_signature:** `3DhcEKSTJPTFzFYAAjCfNHxzvzp8khCymRufL5cQgaHvdrouxYQQvuj4xcfcnNKexMK4WSSiNYDbe2uRrk4bvKTJ`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `GyWok1HCpmXf5jgWjMTzNXye4xMfQashZzs13bkEdZwj`  (fetched at slot 427461506)
- **target / submitted slot:** 427461506  (Jito leader; identity/is_bam not persisted)
- **tip:** 8590 lamports  (market p50 2462, p75 8590 at submit)
- **Jito getInflightBundleStatuses:** `Invalid`  (persisted by the bundle-status poller)
- **lifecycle:**
    - Submitted — 2026-06-19 07:34:57 UTC  (slot 427461506)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:36:03 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Certain)
- **evidence / rationale:** never landed: block engine accepted the bundle (a bundle_id was returned) but getInflightBundleStatuses returned Invalid — the bundle is not in Jito's system / never entered its auction; it did not win. tip 8590 lamports (p50 2462, p75 8590 at submit) (its blockhash later aged to 165 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 6756ms, 2026-06-19 07:36:17 UTC] → **refresh_blockhash, set_tip(10000), resubmit**
        - _AuctionLost (Certain): Block Engine accepted the bundle (bundle_id returned) but getInflightBundleStatuses returned Invalid — the bundle never won its auction. Blockhash age is 182 slots (past the ~150 slot validity window) — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The tip was 8590 lamports, exactly at p75 (8590) at submit time, but the auction was lost. Current p75 is 5175 lamports (down from 8590), and the trend shows rising tips (p50 +260, p75 +697). This is attempt 2. Actions: (1) refresh_blockhash because the blockhash aged to 182 slots while the bundle sat unlanded; (2) set_tip to 10000 lamports to exceed the previous p75 and account for the rising trend, ensuring competitiveness in the next auction; (3) resubmit to compete for the next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:36:17 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 44. `b60bf5ff6270cdff82ecb10205fb9b2bbe008db1fac5c3758cf2f7d62475a07d`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4Jw4DqsYfH6Rbd89Rej69LNttKdjdaa5jx7ig9U7cq3tTe3EB927Djj4dvMB6d167DGVYUukKhYoE56qXLrXRW55`
- **tip_signature:** `31fUEHBCh8FFuyMTfxnwvGEaLM3DC5UH4Sy7DnB7gDg2hmUhbvcxfPoiLbSd4Unjbmyz4vrgA11St28dvAxbHTxr`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `FWofhaSXYBimhgrF3653cEyGitsBHrMew2FaNYNqbUBy`  (fetched at slot 427461530)
- **target / submitted slot:** 427461530  (Jito leader; identity/is_bam not persisted)
- **tip:** 8590 lamports  (market p50 2462, p75 8590 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:35:06 UTC  (slot 427461530)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:36:17 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 8590 lamports (p50 2462, p75 8590 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 175 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7651ms, 2026-06-19 07:36:25 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461530 with tip 8590 lamports (exactly p75 at submit: p50 2462, p75 8590), never landed, now at slot 427461705 — blockhash age 175 slots exceeds the ~150-slot validity window (a downstream symptom of sitting unlanded, not the root cause). Tip 8590 was competitive at submission (matched p75), and current percentiles (p50 2175, p75 5325, rising trend +249/+150 lamports) show the tip remains above p75 now. Attempt 2. The bundle lost its auction or encountered skipped Jito leader slots; the aged blockhash is a consequence, not the cause. Refresh the blockhash to restore validity, then resubmit with the existing tip (still competitive above current p75)._
    - attempt 2 [shadow, baseline, 2026-06-19 07:36:25 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 45. `41c8d108496e1d6b722618a2c33c54e7d2242ad7533c72ee57e788da5d9a8fdc`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4zRb5u2RK2FKHMjF7q4v22h6GMLVVG77UdVnxXXStgVTqyq6NZ54N2CAqzXxXDLZ3zerf5sybszBNAyAAmrWsRyu`
- **tip_signature:** `GGZ7XBvTri3uk8c6unqkz6uGqfn44ZzNQ6tNU8upwd7qdGyuepDLMik2bMAhsKQQDhq7438x9g3zgSgNJyNiHUj`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `Ef5QTZywyatYcw9Ri4dcRRFRuQUkYGQ8KKcvZApuFtY`  (fetched at slot 427461555)
- **target / submitted slot:** 427461555  (Jito leader; identity/is_bam not persisted)
- **tip:** 25000 lamports  (market p50 2462, p75 8590 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:35:17 UTC  (slot 427461555)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:36:25 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 25000 lamports (p50 2462, p75 8590 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 169 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7884ms, 2026-06-19 07:36:33 UTC] → **refresh_blockhash, resubmit**
        - _Classification: AuctionLost (Ambiguous confidence, alternative BundleFailure). Blockhash age: 169 slots (past the ~150-slot validity window — a downstream symptom of the bundle sitting unlanded, not the root cause). Tip: 25000 lamports, well above p50 (2462 at submit, 2175 now) and p75 (8590 at submit, 5325 now) — the tip was competitive and remains so. Attempt: 2. The bundle lost its auction or encountered a skipped Jito leader slot while the blockhash was valid; by the time this was detected the blockhash had aged past validity. Refresh the blockhash to make the bundle eligible for the next Jito leader slot, then resubmit. No tip increase is warranted because 25000 lamports is already 4.7× the current p75 (5325 lamports) and 11.5× the current p50 (2175 lamports). The tip trend shows p50 +249 lamports and p75 +150 lamports (rising), but the existing tip remains dominant. Resubmit with the fresh blockhash to compete in the next auction._
    - attempt 2 [shadow, baseline, 2026-06-19 07:36:33 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 46. `801b9bd9391c946cf62f3ac995ac620d8d05657b08cf438ace48081a4337bd20`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4Pnayd5ks8Hchm8aSW32XbBpsTraaMRdDgZiKQ12rRZ5W8zjWRGK3DDYPYqCmn45vZcx9YgscJa7EUJyXXJxLMpD`
- **tip_signature:** `2yueyxThFbRUKJGCcxfGJUTDS42QX8jW4VMFpYtPvVKtFoZQfkoeYCVoXVcoDQxaSVpyjmW5mrvogsSQ35RRBKVK`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `CNnAYoVso2htXwMMwaP4FbcxyxSjsSwFE2bCsbtER78V`  (fetched at slot 427461578)
- **target / submitted slot:** 427461578  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1666, p75 4478 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:35:25 UTC  (slot 427461578)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:36:33 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1666, p75 4478 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 167 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7463ms, 2026-06-19 07:36:40 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure). Blockhash age is 167 slots, exceeding the ~150-slot validity window — this is a downstream symptom of the bundle sitting unlanded, not the root cause. The bundle was submitted at slot 427461578 with tip 15000 lamports, well above p50 1666 and p75 4478 at submit time. Current percentiles are p50 2175 and p75 5325 (rising trend: p50 +249, p75 +150). The tip remains 2.8x current p75, so competitiveness is not the issue. Attempt 2. The bundle lost its auction or encountered skipped Jito leader slots. Refresh the blockhash to restore validity, then resubmit with the existing tip (15000 lamports is still highly competitive at 2.8x p75). No tip increase needed._
    - attempt 2 [shadow, baseline, 2026-06-19 07:36:40 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 47. `0068d8ccf3e2b30bd6ebfad20a08a3c76a7dd933cf1828cf862b2725ca952cdf`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `64FkoB8gNF82MiLdnbHH5kXqMoeyZ4jbJYcHXon7zBgidbo8XQSZPJsNd8Va56fDpwDFPZ3D3SKkMwdo6zAKQVqV`
- **tip_signature:** `5uCK17vxpshyU2V6pWoh3ct3tbrvXZ4moTvoqM5WkWN5AnVRDvM1GgnQo9wpz7QvdwHFdgrzR1kZFuaziMtzrkZi`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `E84q6xdxigGhEgnTZVmQToouvnhe9PYLEc29rXEFpFeD`  (fetched at slot 427461593)
- **target / submitted slot:** 427461593  (Jito leader; identity/is_bam not persisted)
- **tip:** 10000 lamports  (market p50 1666, p75 4478 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:35:32 UTC  (slot 427461593)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:36:41 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 10000 lamports (p50 1666, p75 4478 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 172 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 7386ms, 2026-06-19 07:36:48 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence, alternative BundleFailure): bundle submitted at slot 427461593 with blockhash fetched at same slot, never landed, now at slot 427461766 — blockhash age 173 slots exceeds the ~150-slot validity window (downstream symptom of sitting unlanded). Tip was 10000 lamports, well above p50 1666 and p75 4478 at submit; current p50 2175 and p75 5325 show modest rise (+249/+150 lamports, rising trend) but tip remains 1.88x current p75, still competitive. Attempt 2. Root cause: lost auction or skipped Jito leader slot while tip was competitive. Blockhash is now stale (aged while unlanded), so refresh_blockhash is required. Tip remains strong relative to percentiles (1.88x p75), so no tip raise needed. Resubmit to compete in next Jito leader slot._
    - attempt 2 [shadow, baseline, 2026-06-19 07:36:48 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 48. `de04b9def31c236bc0cd2071b59e5bc3752b8ab2ea90660ca4bf94af550bdd1b`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `xZifyHjEXS8dXZ6jFsYafSPmzvXAUqZEWLtKbiqeGvnzhFXSF25AiMej2sNR1GXvSdw8DpXce3sM5x1sfAQ5hvs`
- **tip_signature:** `2yo9829Wp8S5pKdaRokSJnK9YbwFtEgp1F7Vd9mrbgJ2M3JySmZWbgRw1a6Qrj4x1MtqpBaUiBkooz57PeUhMoSj`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `5fR7B8LrshmaRd2D9uRFUv4YFBQQRJtdbVWVFpBNfM2W`  (fetched at slot 427461611)
- **target / submitted slot:** 427461611  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 1666, p75 4478 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:35:40 UTC  (slot 427461611)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:36:48 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 15000 lamports (p50 1666, p75 4478 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 172 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)
- **retry agent decisions:**
    - attempt 2 [executed, llm/claude-sonnet-4-5, 5850ms, 2026-06-19 07:36:54 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost (Ambiguous confidence): bundle submitted at slot 427461611 with tip 15000 lamports (well above p50 1666 and p75 4478 at submit) never landed; blockhash now aged 172 slots (past ~150 validity window) — a downstream symptom of sitting unlanded, not the root cause. Tip was competitive at submission (9.0x p50, 3.3x p75) and remains above current p50 2941 and p75 5573, so the auction loss was not due to an uncompetitive tip. Attempt 2 of retrying. Correct action: refresh_blockhash (the blockhash is now stale after aging while the bundle sat unlanded) then resubmit unchanged (the tip is still competitive; no raise needed)._
    - attempt 2 [shadow, baseline, 2026-06-19 07:36:54 UTC] → **refresh_blockhash, resubmit**
        - _AuctionLost_

### 49. `6a805a2b3896cc6c8d6ed5803bf363e60b222a30f189dec8de63a1ba0fe283d4`  (Failed · AuctionLost · Ambiguous(alt: BundleFailure))

- **memo_signature:** `4MZH9hWGfNHcQDQNFFsBNTFohNMfc1cBthxR8piFzTzx68dY7ZuwqimnsskoEV2JNQnBWtCFcwk58E3MeY3nj6Bj`
- **tip_signature:** `3VgYzR4MAYTJWXApPWwgWUHWmXxcVpG2M1sTyYoPsKsnQMyovwMq35avc5oLxyR5gbez6ZYfPTiSxSjiimz1Ephz`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `72aSm6UNbqAMiRZj1s61kQd3Aa8CZd5MTcNSgEZT7Dqn`  (fetched at slot 427461631)
- **target / submitted slot:** 427461631  (Jito leader; identity/is_bam not persisted)
- **tip:** 25000 lamports  (market p50 1666, p75 4478 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:35:47 UTC  (slot 427461631)
    - **Failed (terminal · AuctionLost)** — failure recorded 2026-06-19 07:36:54 UTC
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
- **classification:** **AuctionLost** (Ambiguous(alt: BundleFailure))
- **evidence / rationale:** never landed though the blockhash was valid at submission and the tip was competitive: tip 25000 lamports (p50 1666, p75 4478 at submit) — most likely lost the auction (or a skipped/dropped Jito leader slot); getInflightBundleStatuses was not a definitive Invalid/Failed when last polled (its blockhash later aged to 168 slots, past ~150 — a downstream symptom of sitting unlanded, not the cause)

### 50. `b7926b300be28d570937a0a3b3e7d0243aaf3154d30ad0013b77ba4fbff9f6c4`  (Submitted)

- **memo_signature:** `QHR2SoKq8zgVyqbwCVWHNHGcEyQcjw3McwGNvdHsYjx2ceLiBXkZhQEBCgTiCUpTw9chpMpwVbhg3i8aonaduNk`
- **tip_signature:** `59YQVTtb3LjBkjHbKTinzm3RNXzfadnG3HJ1WDa5Qpdtp1VQoJTq7tjnQmYPAshJzr9zXha9Nt2x1MC7aC7cDk56`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `5ZpA4psHn8efQceQCK7emBx13hxpSku5RSEw5LP5dxBC`  (fetched at slot 427461656)
- **target / submitted slot:** 427461656  (Jito leader; identity/is_bam not persisted)
- **tip:** 12000 lamports  (market p50 1926, p75 5175 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:35:57 UTC  (slot 427461656)
    - _still Submitted — never swept to a terminal state in this run_
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)

### 51. `1d8e4dd9243f647b04a8e94c4547d2b6b6de1daa09b82df6056a526e7fd0d058`  (Submitted)

- **memo_signature:** `3DFtWVnPSsAZmzqDGMZHS27syeBJepF7AhXNVwB3Pe1XhSAG2xXzuZ996SU2na1WFGDdtE4v2QiC4MXrydN6fDTL`
- **tip_signature:** `5DdBJnNw7cKV5PWMj9rwLpcXUhmTgxs8UpRLkgKeWKpbrWVMn8H47iPqeo2ctMeF12hRcnRWhhwK6obeaDktSzxK`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `BJFfiD6AmU6cxx3foG7P4gyZASPiMhyHYzMksRCnHXX8`  (fetched at slot 427461670)
- **target / submitted slot:** 427461670  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 1926, p75 5175 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:36:03 UTC  (slot 427461670)
    - _still Submitted — never swept to a terminal state in this run_
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)

### 52. `c4c7a7925c392e86c5af4ec614e39aa79ba8766f96665bb9cfcef610dfe5dab0`  (Submitted)

- **memo_signature:** `59TTJ4jXd67JwFPs7gm456hkfUWtW22w8Ht1PpBVjeQHtv5quhoWXuuQdhAV8Ucrmb4p35mhLtTjoVVGigLgPbhq`
- **tip_signature:** `5ZKmVzSqS4vKHheCv2SwLKBcRbzZYqGXePirs83csjTxwo39zhgJuQGghCvYcjUbPXUc8XXZjU5NU22ZaYRkJYTu`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `9mPfSQyzZxvFFVoyU7kRPitYvy6wQ88fNEge4HGtnHSU`  (fetched at slot 427461687)
- **target / submitted slot:** 427461687  (Jito leader; identity/is_bam not persisted)
- **tip:** 17000 lamports  (market p50 1926, p75 5175 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:36:10 UTC  (slot 427461687)
    - _still Submitted — never swept to a terminal state in this run_
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)

### 53. `b1ab8bd3e311a6dbec76a17dc357228f56d85b717e1b831937750b8a5f378248`  (Submitted)

- **memo_signature:** `5dHUQWVxCqscjaMuwZcfFFfJtCzA6PFBg8rLobxMMR4wxvKwKrBZxTqAzr8rA9zns4cszuN9skkhpJAQWqThtHMD`
- **tip_signature:** `nZFDyitVxNSo9yCvRr3UshbznV8Sqkp4VRvtW72f9RssXbQ5M4pSY6eJsfYqseBkvT8dPcFhg8MjitVBP5ktSg3`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `yoCFkydp8YXemgpDrqKUPQKca1Kd7ZATPjQFQaXQYgc`  (fetched at slot 427461704)
- **target / submitted slot:** 427461704  (Jito leader; identity/is_bam not persisted)
- **tip:** 10000 lamports  (market p50 1926, p75 5175 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:36:17 UTC  (slot 427461704)
    - _still Submitted — never swept to a terminal state in this run_
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)

### 54. `dcc4d06e4e580afe8c1bfbc572ff26b48a312ef507a5b6046448f119c31ad36e`  (Submitted)

- **memo_signature:** `4hamK3WJriWxaFCbe8HbPSG7Ujh2veMU6h76beFdYf89s2r9WmJCi4aXVJ8iVujoRCad7t9ZY2Fi3oKVgy5L6JRy`
- **tip_signature:** `UeGuobj6dwzQ5BEJcDwBn77rXbCDq8ug2cvbx4DcFmoZ9AoJaTshDjeiQ6pRtKeiuV1GVBWDFsRC9aZ2t1WrpfX`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `Dyvad9x3AtCrKeNchT1o8ckwr2u6vTWN5jFWAjRTBV2f`  (fetched at slot 427461723)
- **target / submitted slot:** 427461723  (Jito leader; identity/is_bam not persisted)
- **tip:** 8590 lamports  (market p50 2175, p75 5325 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:36:25 UTC  (slot 427461723)
    - _still Submitted — never swept to a terminal state in this run_
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)

### 55. `b2b110e6bd47bc3c6b1bcd87844d60bfe7e3d1c4f5f78d733453e335da6242c1`  (Submitted)

- **memo_signature:** `2Z1mtzsN8Xc3ATTrn5gmB6RUJgFSqd6pn1prqzariX7y1zbHdDJ75hpj2sMH4jJqRQq3Q9PEFaznGwjbKNqTYJ56`
- **tip_signature:** `5Y9djm6wrVJAspKoD7esdZtfvsSLZaMLkzLUKXHMnagc3ATdFQrkk1raYAyYKbLCDrXvfpFoaBRSLwnd7m7EhHky`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `9Yj9fY2VGD5ns7Q6AGGUzYw8sn2MQCcdHYVPwRafr7Rx`  (fetched at slot 427461744)
- **target / submitted slot:** 427461744  (Jito leader; identity/is_bam not persisted)
- **tip:** 25000 lamports  (market p50 2175, p75 5325 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:36:33 UTC  (slot 427461744)
    - _still Submitted — never swept to a terminal state in this run_
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)

### 56. `006a040229c5dbfb9b5fecac250ec80c2eff0ad4cb0b26c6a5f75d7598e66c4c`  (Submitted)

- **memo_signature:** `5Z32bCQc3SJA9bTRaAvL7vFpgUfns41NNzcFvX8FQKdAF14hj7dZFeytD11bTA9BUD7EgdJtjsWwZfdBWtZpV96x`
- **tip_signature:** `jBT24tmkqQeJYx3ktj6JA1q5yq61u7LTFQfGXyfBKLFGUS7xpXNK1dwJRALt2qwEZPYcXLfLd3xzguQXxHf3sjE`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `5pFGYdtWkz7sq2XKdGehEP2yPQixkQzmbgkw9QoT9R9F`  (fetched at slot 427461765)
- **target / submitted slot:** 427461765  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 2175, p75 5325 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:36:40 UTC  (slot 427461765)
    - _still Submitted — never swept to a terminal state in this run_
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)

### 57. `6777c6add9897aadf131fcafe48e21468efab6bcb355727b91059418edb52631`  (Submitted)

- **memo_signature:** `2f1LFGtkutmmAoCJUCrLxuQuZzDcKQ17HpSgYmpNtE4hCqwH61E3ou4fjcDDgsS9Y7TuFZ2vRYacDb9Agwgbabxe`
- **tip_signature:** `4q6CGLZzcZp6dQSC2JKEaUvVGUdiipFAs1YpfxDRMdrSfhd7gqVVsVf8unqDoDKiGJwM49otgEF3bbX2L5So6u9g`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `9ny7it74SCZ2VTAhkrfVZTyBGtzCFkntTZd7LSrimue7`  (fetched at slot 427461783)
- **target / submitted slot:** 427461783  (Jito leader; identity/is_bam not persisted)
- **tip:** 10000 lamports  (market p50 2175, p75 5325 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:36:48 UTC  (slot 427461783)
    - _still Submitted — never swept to a terminal state in this run_
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)

### 58. `68199943b93df03f715f515a2b0ea2775452fd2b93235402713a8955ef42f85a`  (Submitted)

- **memo_signature:** `3AU2PEE4fVyfrtcYsULH2yyUUsdUbXC1srA46Ct9pZSZx4oQzh1LA3HL5z8jQzV8L7tSf4sswegjfm4pszC8ioGn`
- **tip_signature:** `34h2WLeHZqDvNyEy5mghZPnqy4mo4ksJiAGEdT86QTt9exp2N12dgK5rgj5LhuD3G87NJwGnyr99y8wDe7Wn6LtR`
- **tip_account:** `HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe`
- **blockhash:** `Ea5egqTXfnbTZza6uLXog5NeSwR9NKyQ1ss1eP6NmUw5`  (fetched at slot 427461799)
- **target / submitted slot:** 427461799  (Jito leader; identity/is_bam not persisted)
- **tip:** 15000 lamports  (market p50 2941, p75 5573 at submit)
- **lifecycle:**
    - Submitted — 2026-06-19 07:36:54 UTC  (slot 427461799)
    - _still Submitted — never swept to a terminal state in this run_
- **latency deltas:** n/a — bundle never reached Processed (never won its auction)
