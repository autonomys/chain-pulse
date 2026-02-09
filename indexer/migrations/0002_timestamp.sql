DELETE
FROM indexer.metadata;

ALTER TABLE IF EXISTS indexer.xdm_transfers
    ADD COLUMN IF NOT EXISTS transfer_initiated_on_src_at    TIMESTAMP WITH TIME ZONE,
    ADD COLUMN IF NOT EXISTS transfer_executed_on_dst_at     TIMESTAMP WITH TIME ZONE,
    ADD COLUMN IF NOT EXISTS transfer_acknowledged_on_src_at TIMESTAMP WITH TIME ZONE;
