// TODO:
#![allow(dead_code)]

use crate::error::Error;
use shared::subspace::HashAndNumber;
use sqlx::PgPool;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use subxt::utils::H256;

#[derive(Clone)]
pub(crate) struct Db {
    pub(crate) pool: Arc<PgPool>,
}

impl Db {
    pub(crate) async fn new(db_url: &str, migrations_path: &str) -> Result<Self, Error> {
        let pg_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(50)
            .acquire_slow_threshold(Duration::from_secs(10))
            .acquire_timeout(Duration::from_secs(60))
            .connect(db_url)
            .await?;

        let mut migrator = sqlx::migrate::Migrator::new(Path::new(migrations_path)).await?;
        migrator.set_ignore_missing(true);
        migrator.run(&pg_pool).await?;
        Ok(Db {
            pool: Arc::new(pg_pool),
        })
    }

    pub(crate) async fn set_last_processed_block(
        &self,
        process: &str,
        block: HashAndNumber,
    ) -> Result<(), Error> {
        let HashAndNumber { hash, number } = block;
        let _ = sqlx::query(
            r#"
        INSERT INTO indexer.metadata as m (process, processed_block_number, processed_block_hash)
        VALUES ($1, $2, $3)
        ON CONFLICT (process) DO UPDATE
        SET processed_block_number = EXCLUDED.processed_block_number,
            processed_block_hash = EXCLUDED.processed_block_hash
        "#,
        )
        .bind(process)
        .bind(number as i64)
        .bind(hash.to_string())
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn get_last_processed_block(
        &self,
        process: &str,
    ) -> Result<HashAndNumber, Error> {
        let (number, hash) = sqlx::query_scalar::<_, (i64, String)>(
            r#"
            select processed_block_number, processed_block_hash from indexer.metadata WHERE process = $1
            "#,
        )
        .bind(process)
        .fetch_one(&*self.pool)
        .await?;

        Ok(HashAndNumber {
            hash: H256::from_str(&hash).expect("invalid hash"),
            number: number as u32,
        })
    }
}
