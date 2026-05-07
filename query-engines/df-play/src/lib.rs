use datafusion::prelude::SessionContext;

pub mod basic;
pub mod delta;
pub mod filter_pushdown;
pub mod tree_node_;

fn build_ctx() -> SessionContext {
    SessionContext::new()
}

async fn register_parquet_table(ctx: &SessionContext, table_name: &str) -> anyhow::Result<()> {
    ctx.register_parquet(
        table_name,
        "/home/ak/github/open-source/query-engines/df-play/tables",
        Default::default(),
    )
    .await?;
    Ok(())
}
