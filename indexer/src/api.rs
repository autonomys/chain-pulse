use crate::WebState;
use crate::error::Error;
use crate::staking::CHECKPOINT_KEY as STAKING_CHECKPOINT_KEY;
use crate::storage::{OperatorRow, SharePriceRow};
use crate::types::{ChainId, DomainId};
use crate::xdm::get_xdm_processor_key;
use actix_web::{Responder, get, web};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use shared::subspace::BlockNumber;
use tokio::try_join;

const MAX_RECENT_TRANSFERS: u64 = 10;
const MAX_SHARE_PRICE_LIMIT: i64 = 50;

pub(crate) fn health_config(cfg: &mut web::ServiceConfig) {
    cfg.service(health_check);
}

#[derive(Serialize)]
pub(crate) struct Health {
    consensus_processed_block_number: BlockNumber,
    auto_evm_processed_block_number: BlockNumber,
    staking_processed_block_number: BlockNumber,
}

#[get("/health")]
async fn health_check(data: web::Data<WebState>) -> Result<impl Responder, Error> {
    let cn_key = get_xdm_processor_key(&ChainId::Consensus);
    let aen_key = get_xdm_processor_key(&ChainId::Domain(DomainId(0)));
    let (cn, aen, staking) = try_join!(
        data.db.get_last_processed_block(&cn_key),
        data.db.get_last_processed_block(&aen_key),
        data.db.get_last_processed_block(STAKING_CHECKPOINT_KEY),
    )?;
    Ok(web::Json(Health {
        consensus_processed_block_number: cn,
        auto_evm_processed_block_number: aen,
        staking_processed_block_number: staking,
    }))
}

pub(crate) fn xdm_config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/v1/xdm")
            .service(xdm_address_transfers)
            .service(recent_xdm_transfers),
    );
}

#[derive(Serialize)]
pub(crate) struct BlockDetails {
    pub(crate) block_number: BlockNumber,
    pub(crate) block_hash: String,
    pub(crate) block_time: DateTime<Utc>,
}

#[derive(Serialize)]
pub(crate) struct MaybeBlockDetails(pub(crate) Option<BlockDetails>);

#[derive(Serialize)]
pub(crate) struct XdmTransfer {
    pub(crate) src_chain: String,
    pub(crate) dst_chain: String,
    pub(crate) channel_id: String,
    pub(crate) nonce: String,
    pub(crate) sender: Option<String>,
    pub(crate) receiver: Option<String>,
    pub(crate) amount: Option<Decimal>,
    pub(crate) initiated_src_block: MaybeBlockDetails,
    pub(crate) executed_dst_block: MaybeBlockDetails,
    pub(crate) acknowledged_src_block: MaybeBlockDetails,
    pub(crate) transfer_successful: Option<bool>,
}

#[get("/transfers/{address}")]
async fn xdm_address_transfers(
    data: web::Data<WebState>,
    path: web::Path<String>,
) -> Result<impl Responder, Error> {
    let address = path.into_inner();
    let decimal_scale = data.decimal_scale;
    let transfers: Vec<XdmTransfer> = data
        .db
        .get_xdm_transfer_for_address(&address)
        .await?
        .into_iter()
        .map(|transfer| (decimal_scale, transfer).into())
        .collect();
    Ok(web::Json(transfers))
}

#[derive(Deserialize)]
struct RecentXdmTransfersQuery {
    #[serde(default = "max_recent_transfers_limit")]
    limit: u64,
}

fn max_recent_transfers_limit() -> u64 {
    MAX_RECENT_TRANSFERS
}

#[get("/recent")]
async fn recent_xdm_transfers(
    data: web::Data<WebState>,
    info: web::Query<RecentXdmTransfersQuery>,
) -> Result<impl Responder, Error> {
    let RecentXdmTransfersQuery { limit } = info.into_inner();
    let limit = if limit > MAX_RECENT_TRANSFERS {
        MAX_RECENT_TRANSFERS
    } else {
        limit
    };
    let decimal_scale = data.decimal_scale;
    let transfers: Vec<XdmTransfer> = data
        .db
        .get_recent_xdm_transfers(limit)
        .await?
        .into_iter()
        .map(|transfer| (decimal_scale, transfer).into())
        .collect();
    Ok(web::Json(transfers))
}

pub(crate) fn staking_config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/v1/staking").service(
            web::scope("/operators").service(get_all_operators).service(
                web::scope("/{operator_id}")
                    .service(get_operator)
                    .service(operator_share_prices)
                    .service(operator_nominator_count),
            ),
        ),
    );
}

#[derive(Serialize)]
struct OperatorResponse {
    id: String,
    domain_id: String,
    owner_account: String,
    signing_key: String,
    minimum_nominator_stake: String,
    nomination_tax: u8,
    total_stake: String,
    total_shares: String,
    total_storage_fee_deposit: String,
    status: String,
    nominator_count: i64,
}

fn map_operator_row(row: OperatorRow) -> OperatorResponse {
    OperatorResponse {
        id: row.operator_id,
        domain_id: row.domain_id,
        owner_account: row.owner_account,
        signing_key: row.signing_key,
        minimum_nominator_stake: row.minimum_nominator_stake,
        nomination_tax: row.nomination_tax as u8,
        total_stake: row.total_stake,
        total_shares: row.total_shares,
        total_storage_fee_deposit: row.total_storage_fee_deposit,
        status: row.status,
        nominator_count: row.nominator_count,
    }
}

#[get("")]
async fn get_all_operators(data: web::Data<WebState>) -> Result<impl Responder, Error> {
    let operators: Vec<OperatorResponse> = data
        .db
        .get_all_operators()
        .await?
        .into_iter()
        .map(map_operator_row)
        .collect();
    Ok(web::Json(operators))
}

#[get("")]
async fn get_operator(
    data: web::Data<WebState>,
    path: web::Path<i64>,
) -> Result<impl Responder, Error> {
    let operator_id = path.into_inner();
    match data.db.get_operator(operator_id).await? {
        Some(row) => Ok(web::Json(map_operator_row(row))),
        None => Err(Error::NotFound(format!("Operator {operator_id} not found"))),
    }
}

#[derive(Deserialize)]
struct SharePricesQuery {
    #[serde(default = "default_share_price_limit")]
    limit: i64,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
}

fn default_share_price_limit() -> i64 {
    MAX_SHARE_PRICE_LIMIT
}

#[get("/share-prices")]
async fn operator_share_prices(
    data: web::Data<WebState>,
    path: web::Path<i64>,
    info: web::Query<SharePricesQuery>,
) -> Result<impl Responder, Error> {
    let operator_id = path.into_inner();
    let SharePricesQuery {
        limit,
        since,
        until,
    } = info.into_inner();
    let limit = limit.clamp(1, MAX_SHARE_PRICE_LIMIT);
    let rows: Vec<SharePriceRow> = match (since, until) {
        (Some(since), _) => {
            data.db
                .get_share_prices_since(operator_id, since, limit)
                .await?
        }
        (_, Some(until)) => {
            data.db
                .get_share_prices_until(operator_id, until, limit)
                .await?
        }
        _ => data.db.get_share_prices_latest(operator_id, limit).await?,
    };
    let response: Vec<_> = rows
        .into_iter()
        .map(|r| SharePriceResponse {
            operator_id: r.operator_id,
            domain_id: r.domain_id,
            epoch_index: r.epoch_index,
            share_price: r.share_price,
            total_stake: r.total_stake,
            total_shares: r.total_shares,
            block_height: r.block_height,
            timestamp: r.block_time,
        })
        .collect();
    Ok(web::Json(response))
}

#[derive(Serialize)]
struct SharePriceResponse {
    operator_id: String,
    domain_id: String,
    epoch_index: i64,
    share_price: String,
    total_stake: String,
    total_shares: String,
    block_height: String,
    timestamp: DateTime<Utc>,
}

#[derive(Serialize)]
struct NominatorCountResponse {
    operator_id: i64,
    active_count: i64,
}

#[get("/nominators/count")]
async fn operator_nominator_count(
    data: web::Data<WebState>,
    path: web::Path<i64>,
) -> Result<impl Responder, Error> {
    let operator_id = path.into_inner();
    let active_count = data.db.get_active_nominator_count(operator_id).await?;
    Ok(web::Json(NominatorCountResponse {
        operator_id,
        active_count,
    }))
}
