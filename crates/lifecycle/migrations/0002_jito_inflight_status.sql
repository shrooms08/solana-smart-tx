-- Last Jito `getInflightBundleStatuses` verdict observed for a bundle, recorded
-- by the bundle-status poller: 'Invalid' | 'Pending' | 'Failed' | 'Landed' (or an
-- unrecognized string). NULL until polled. Plumbed into the never-landed
-- classifier so an accepted-but-Invalid bundle is classified AuctionLost rather
-- than ExpiredBlockhash (the aged blockhash being only a downstream symptom).
ALTER TABLE bundle_submissions ADD COLUMN jito_inflight_status TEXT;
