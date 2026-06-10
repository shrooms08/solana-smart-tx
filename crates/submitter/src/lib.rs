//! Jito bundle construction + `sendBundle`.
//!
//! Builds tip-bearing bundles and submits them to a Jito block-engine region
//! via the JSON-RPC SDK.
//!
//! TODO: assemble bundles (tip ix + payload txs), sign, and call `sendBundle`.

use serde::{Deserialize, Serialize};

/// Where to submit bundles.
#[derive(Debug, Clone)]
pub struct SubmitterConfig {
    /// Jito block-engine region base URL.
    pub block_engine_url: String,
}

/// Identifier returned by the block engine for a submitted bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleId(pub String);

/// A bundle ready (or being prepared) for submission.
///
/// TODO: replace with real `VersionedTransaction` payload + tip accounting.
#[derive(Debug, Default, Clone)]
pub struct Bundle {
    /// Tip amount in lamports.
    pub tip_lamports: u64,
    // TODO: signed transactions, tip account, target leader slot.
}

/// Submits bundles to the Jito block engine.
///
/// TODO: own the `jito_sdk_rust` client.
#[derive(Debug)]
pub struct BundleSubmitter {
    _config: SubmitterConfig,
}

impl BundleSubmitter {
    pub fn new(config: SubmitterConfig) -> Self {
        Self { _config: config }
    }

    /// Submit a bundle, returning its [`BundleId`].
    ///
    /// TODO: implement via the Jito SDK `sendBundle` call.
    pub async fn send_bundle(&self, _bundle: Bundle) -> anyhow::Result<BundleId> {
        // Test-only failure injection, compiled out unless the feature is on.
        #[cfg(feature = "fault-injection")]
        {
            if fault::should_drop() {
                anyhow::bail!("fault-injection: simulated dropped bundle");
            }
        }

        // TODO: build + sign + send the bundle.
        todo!("sendBundle not implemented yet")
    }
}

/// Test-only failure-injection helpers (compiled only with `fault-injection`).
#[cfg(feature = "fault-injection")]
pub mod fault {
    /// Whether the next submission should be forced to fail.
    ///
    /// TODO: drive from env/config so tests can steer the injected failures.
    pub fn should_drop() -> bool {
        false
    }
}
