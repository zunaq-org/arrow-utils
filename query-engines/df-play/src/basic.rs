use deltalake::arrow::array::RecordBatch;

pub async fn build_simple() -> anyhow::Result<()> {
    let ctx = crate::build_ctx();
    crate::register_parquet_table(&ctx, "sq_avg_monthly_sales").await?;

    // let sql = "select * from sq_avg_monthly_sales";
    // let logic
    // al_plan = ctx.state().create_logical_plan(sql).await?;

    // println!("Plan Before: {}\n\n\n", logical_plan.display_indent().to_string());
    // print!("Schema Before: {}\n\n\n", logical_plan.display_indent_schema().to_string());

    let df = ctx
        .sql("select * from sq_avg_monthly_sales limit 50")
        .await?;

    println!("{}", df.schema());

    let batches = df.collect().await?;
    println!("Len: {}", batches.len());

    println!(
        "{}",
        datafusion::arrow::util::pretty::pretty_format_batches(batches.as_slice())?
    );

    // let df = df.filter(col("productnum").eq(lit("3723 LSPC POLYNT G.P. POLYEST RESIN DRUM")))?;
    // let logical_plan = df.logical_plan();
    // println!("Plan After: {}", logical_plan.display_indent().to_string());
    //
    // print!("Schema: {}", df.logical_plan().display_indent_schema().to_string());
    Ok(())
}

pub async fn run_query(query: &str) -> anyhow::Result<Vec<RecordBatch>> {
    let ctx = crate::build_ctx();
    crate::register_parquet_table(&ctx, "sq_avg_monthly_sales").await?;
    crate::register_parquet_table(&ctx, "sales_order_details").await?;
    crate::register_parquet_table(&ctx, "customer_group").await?;
    let df = ctx.sql(query).await?;
    Ok(df.collect().await?)
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn simple_query_test() -> anyhow::Result<()> {
        let _ = super::build_simple().await?;
        Ok(())
    }

    #[tokio::test]
    async fn run_query_sales_order_details() {
        let query = "select productnum from sales_order_details limit 10";
        let result = super::run_query(query).await.expect("something went wrong");
        let pretty_result =
            datafusion::arrow::util::pretty::pretty_format_batches(result.as_slice())
                .expect("err in pretty");

        println!("result: {}", pretty_result);
    }
}
