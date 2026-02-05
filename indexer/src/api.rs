use crate::WebState;
use crate::error::Error;
use crate::types::{ChainId, DomainId};
use crate::xdm::get_processor_key;
use actix_web::{Responder, get, web};
use rust_decimal::Decimal;
use serde::Serialize;
use shared::subspace::BlockNumber;
use tokio::try_join;

pub(crate) fn health_config(cfg: &mut web::ServiceConfig) {
    cfg.service(health_check);
}

#[derive(Serialize)]
pub(crate) struct Health {
    consensus_processed_block_number: BlockNumber,
    auto_evm_processed_block_number: BlockNumber,
}

#[get("/health")]
async fn health_check(data: web::Data<WebState>) -> Result<impl Responder, Error> {
    let cn_key = get_processor_key(&ChainId::Consensus);
    let aen_key = get_processor_key(&ChainId::Domain(DomainId(0)));
    let (cn, aen) = try_join!(
        data.db.get_last_processed_block(&cn_key),
        data.db.get_last_processed_block(&aen_key),
    )?;

    Ok(web::Json(Health {
        consensus_processed_block_number: cn,
        auto_evm_processed_block_number: aen,
    }))
}

pub(crate) fn xdm_config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/v1/xdm").service(xdm_address_transfers));
}

#[derive(Serialize)]
pub(crate) struct BlockDetails {
    pub(crate) block_number: BlockNumber,
    pub(crate) block_hash: String,
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
    let transfers = data
        .db
        .get_xdm_transfer_for_address(&address)
        .await?
        .into_iter()
        .map(|transfer| (decimal_scale, transfer).into())
        .collect::<Vec<XdmTransfer>>();
    Ok(web::Json(transfers))
}
