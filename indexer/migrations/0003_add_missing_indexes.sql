-- Index for get_recent_xdm_transfers: ORDER BY transfer_initiated_on_src_at DESC LIMIT N
CREATE INDEX IF NOT EXISTS xdm_transfers_initiated_at_idx
    ON indexer.xdm_transfers (transfer_initiated_on_src_at DESC)
    WHERE transfer_initiated_on_src_at IS NOT NULL;

-- Replace single-column sender/receiver indexes with composite ones
-- that cover both the WHERE filter and ORDER BY in get_xdm_transfer_for_address
DROP INDEX IF EXISTS indexer.xdm_transfers_sender_idx;
DROP INDEX IF EXISTS indexer.xdm_transfers_receiver_idx;

CREATE INDEX IF NOT EXISTS xdm_transfers_sender_initiated_at_idx
    ON indexer.xdm_transfers (sender, transfer_initiated_on_src_at DESC)
    WHERE sender IS NOT NULL;

CREATE INDEX IF NOT EXISTS xdm_transfers_receiver_initiated_at_idx
    ON indexer.xdm_transfers (receiver, transfer_initiated_on_src_at DESC)
    WHERE receiver IS NOT NULL;
