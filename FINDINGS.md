## Findings

The central operational finding of this project came from driving the system to
the point where every variable under the operator's control is provably correct,
and then asking what remained.

### What was eliminated

Over the course of building and debugging on mainnet, each of the following was
ruled out as a reason a bundle fails to land — each by direct measurement, not
assumption:

- **Bundle construction** — decoded from the exact bytes on the wire: a valid
  tip transfer to a fetched Jito tip account, correct ordering, shared blockhash.
- **Execution** — both transactions pass `simulateTransaction` with signature
  verification on and blockhash replacement off.
- **Wallet funding** — verified on-chain.
- **Tip competitiveness** — tested from p75 up to the configured maximum, far
  above the live tip-floor percentiles.
- **Submission timing** — submitting one to two slots ahead of the target Jito
  leader, with total in-window latency around 200ms and zero slot drift, after
  the latency work described in ARCHITECTURE.md §5.
- **Authentication** — submissions carry a Jito JSON-RPC UUID in the
  `x-jito-auth` header (anonymous submissions, per Jito support, rarely win).
- **Region routing** — identical results on the global and Amsterdam block
  engines.
- **BAM priority fees** — on BAM leaders, a compute-budget priority fee paired
  with an explicit compute-unit limit, to compete in the (tips + priority
  fees)/CU auction rather than tips/CU alone.

### What remained

With all of the above correct, the block engine consistently accepts the bundle
(returns a `bundle_id`) and then reports it as `Invalid` through
`getInflightBundleStatuses` — meaning it did not win its auction and the record
was discarded. This was confirmed directly with Jito support: a returned bundle
id means only that the bundle was received, and Jito retains information only for
bundles that win.

The conclusion the evidence supports is that **a bundle carrying no economic
activity — a memo and a self-transfer, paying a tip out of pocket — does not win
a Jito auction against the real arbitrage and swap flow it competes with**, and
that no amount of tip, priority fee, authentication, timing precision, or region
selection changes that, because the auction rewards the economic value a bundle
brings to the block, not the bundle's mere presence. The Block Engine auction
scores roughly on tips per compute unit and the BAM Node auction on (tips +
priority fees) per compute unit; in both, a contentless bundle is competing on
an axis where it has no structural advantage and a real MEV bundle does.

This is not a defect in the system. Every component does its job: the stream
stays connected, the leader windows fire ahead of Jito leaders, the tips are
priced from live data, the bundles are well-formed and authenticated and
on-time, the lifecycle is tracked accurately, the failures are classified
correctly as auction losses, and the agent reasons sensibly about each one. The
finding is about the auction itself, and it is one that only becomes visible by
instrumenting the system to tell the truth about its own behaviour — capturing
raw responses, decoding bundles on the wire, querying Jito's own status API, and
reading the auction-priority rules from the source.

### What landing a bundle would require

The natural extension, beyond the scope of this infrastructure challenge, is to
give the bundle genuine economic content — a real on-chain action (an arbitrage,
a swap, a state change someone values) whose profit funds the tip. At that point
the bundle competes on the same axis as the flow that currently beats it. The
infrastructure built here — streaming, leader targeting, live tip pricing,
low-latency authenticated submission, lifecycle tracking, failure
classification, and the agent decision layer — is exactly what such a strategy
would sit on top of. The hard part of a smart transaction stack is the
machinery; the machinery is what this project built and verified.
