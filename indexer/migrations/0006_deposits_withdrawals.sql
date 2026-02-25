CREATE TABLE IF NOT EXISTS indexer.nominator_deposits
(
    operator_id   BIGINT          NOT NULL,
    address       TEXT            NOT NULL,
    amount        NUMERIC(39, 0) NOT NULL,
    storage_fee   NUMERIC(39, 0) NOT NULL DEFAULT 0,
    block_height  BIGINT          NOT NULL,
    block_time    TIMESTAMPTZ     NOT NULL
);

CREATE INDEX IF NOT EXISTS deposits_addr_op_time_idx
    ON indexer.nominator_deposits (address, operator_id, block_time DESC);

CREATE TABLE IF NOT EXISTS indexer.nominator_withdrawals
(
    operator_id        BIGINT          NOT NULL,
    address            TEXT            NOT NULL,
    shares             NUMERIC(39, 0) NOT NULL DEFAULT 0,
    amount             NUMERIC(39, 0) NOT NULL DEFAULT 0,
    storage_fee_refund NUMERIC(39, 0) NOT NULL DEFAULT 0,
    block_height       BIGINT          NOT NULL,
    block_time         TIMESTAMPTZ     NOT NULL
);

CREATE INDEX IF NOT EXISTS withdrawals_addr_op_time_idx
    ON indexer.nominator_withdrawals (address, operator_id, block_time DESC);

-- Reset the staking processor checkpoint so it reindexes from genesis.
-- This captures all historical deposit/withdrawal events.
DELETE FROM indexer.metadata WHERE process = 'staking_processor_Consensus';
