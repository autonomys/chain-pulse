CREATE TABLE IF NOT EXISTS indexer.operator_epoch_share_prices
(
    operator_id  BIGINT          NOT NULL,
    domain_id    INTEGER         NOT NULL,
    epoch_index  BIGINT          NOT NULL,
    share_price  NUMERIC(28, 18) NOT NULL,
    total_stake  NUMERIC(39, 0)  NOT NULL,
    total_shares NUMERIC(39, 0)  NOT NULL,
    block_height BIGINT          NOT NULL,
    block_time   TIMESTAMPTZ     NOT NULL,
    PRIMARY KEY (operator_id, domain_id, epoch_index)
);

CREATE INDEX IF NOT EXISTS op_epoch_sp_time_idx
    ON indexer.operator_epoch_share_prices (operator_id, block_time DESC);

CREATE TABLE IF NOT EXISTS indexer.nominators
(
    operator_id  BIGINT NOT NULL,
    address      TEXT   NOT NULL,
    status       TEXT   NOT NULL DEFAULT 'active',
    block_height BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (operator_id, address)
);

CREATE INDEX IF NOT EXISTS nominators_operator_status_idx
    ON indexer.nominators (operator_id, status);

CREATE TABLE IF NOT EXISTS indexer.operators
(
    operator_id               BIGINT         NOT NULL PRIMARY KEY,
    domain_id                 INTEGER        NOT NULL,
    owner_account             TEXT           NOT NULL,
    signing_key               TEXT           NOT NULL,
    minimum_nominator_stake   NUMERIC(39, 0) NOT NULL,
    nomination_tax            SMALLINT       NOT NULL,
    status                    TEXT           NOT NULL DEFAULT 'registered',
    total_stake               NUMERIC(39, 0) NOT NULL DEFAULT 0,
    total_shares              NUMERIC(39, 0) NOT NULL DEFAULT 0,
    total_storage_fee_deposit NUMERIC(39, 0) NOT NULL DEFAULT 0,
    updated_at                TIMESTAMPTZ    NOT NULL DEFAULT now()
);
