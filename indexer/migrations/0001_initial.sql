CREATE SCHEMA IF NOT EXISTS indexer;

CREATE TABLE IF NOT EXISTS indexer.metadata
(
    process                TEXT PRIMARY KEY NOT NULL,
    processed_block_number BIGINT           NOT NULL
);

CREATE TABLE IF NOT EXISTS indexer.xdm_transfers
(
    src_chain                                 TEXT           NOT NULL,
    dst_chain                                 TEXT           NOT NULL,
    channel_id                                NUMERIC(78, 0) NOT NULL,
    nonce                                     NUMERIC(78, 0) NOT NULL,
-- initiated data on src_chain
    sender                                    TEXT,
    receiver                                  TEXT,
    amount                                    NUMERIC(39, 0),
    transfer_initiated_block_number           BIGINT,
    transfer_initiated_block_hash             TEXT,
-- execution on dst_chain
    transfer_executed_on_dst_block_number     BIGINT,
    transfer_executed_on_dst_block_hash       TEXT,
-- acknowledgement on src_chain
    transfer_acknowledged_on_src_block_number BIGINT,
    transfer_acknowledged_on_src_block_hash   TEXT,
    transfer_successful                       boolean,
    PRIMARY KEY (src_chain, dst_chain, channel_id, nonce)
);

CREATE INDEX IF NOT EXISTS xdm_transfers_sender_idx
    ON indexer.xdm_transfers (sender)
    WHERE sender IS NOT NULL;

CREATE INDEX IF NOT EXISTS xdm_transfers_receiver_idx
    ON indexer.xdm_transfers (receiver)
    WHERE receiver IS NOT NULL;

CREATE INDEX IF NOT EXISTS xdm_transfers_src_chain_idx
    ON indexer.xdm_transfers (src_chain);

CREATE INDEX IF NOT EXISTS xdm_transfers_dst_chain_idx
    ON indexer.xdm_transfers (dst_chain);