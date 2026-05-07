use anyhow::anyhow;
use deltalake::DeltaTable;
use deltalake::datafusion::prelude::SessionContext;
use deltalake::gcp::register_handlers;
use deltalake::parquet::arrow::ArrowWriter;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub async fn download_table(table_name: &str, table_path_prefix: &str) -> anyhow::Result<()> {
    let (ctx, _tables) = register_gcs_table(table_name, table_path_prefix).await?;

    let query = format!("select * from {}", table_name);
    let df = ctx.sql(query.as_str()).await?;
    let schema = df.schema().inner().clone();
    println!("data loaded");

    println!("creating file");
    let output_path = format!(
        "/home/ak/github/open-source/query-engines/df-play/tables/{}.parquet",
        table_name
    );
    let table_file = std::fs::File::create(output_path)?;

    println!("file created");

    let mut writer = ArrowWriter::try_new(table_file, schema, None)?;

    let batches = df.collect().await?;

    for batch in batches {
        println!("writing batch");
        writer.write(&batch)?;
    }

    writer.close()?;

    Ok(())
}

// Optimized version without needing to pass runtime handle
pub async fn register_gcs_table(
    table_name: &str,
    table_path_prefix: &str,
) -> anyhow::Result<(SessionContext, DeltaTable)> {
    let df_ctx = SessionContext::new();
    let (bucket_name, _account_key, delta_creds) = creds();

    let table_path = format!("gs://{}/{}/{}", bucket_name, table_path_prefix, table_name);

    let table =
        register_table_optimized(&df_ctx, table_name.to_string(), table_path, &delta_creds).await?;

    Ok((df_ctx, table))
}

// Optimized register_table that uses the current runtime
async fn register_table_optimized(
    df_ctx: &SessionContext,
    table_name: String,
    path: String,
    storage_options: &HashMap<String, String>,
) -> anyhow::Result<DeltaTable> {
    register_handlers(None);

    // Check if table already exists
    if df_ctx.table_exist(&table_name)? {
        return Err(anyhow::anyhow!("table-exists in df ctx"));
    }

    let t_start_time = Instant::now();

    println!("registering table: {}", table_name);

    // Use current runtime instead of passing handle
    let table = deltalake::DeltaTableBuilder::from_uri(path)
        .with_storage_options(storage_options.clone())
        .with_allow_http(true)
        // Let Delta Lake use the current runtime context - more efficient
        .load()
        .await
        .map_err(|err| {
            anyhow!(
                "Registering table {:?} failed due to the error [{:?}]",
                table_name,
                err.to_string()
            )
        })?;

    println!(
        "time for load: {} time: {}ms",
        table_name,
        t_start_time.elapsed().as_millis()
    );

    df_ctx
        .register_table(&table_name, Arc::new(table.clone()))
        .map_err(|err| {
            anyhow!(
                "Registering table {:?} failed due to the error [{:?}]",
                table_name,
                err.to_string()
            )
        })?;

    println!(
        "time for load and register: {} time: {}ms",
        table_name,
        t_start_time.elapsed().as_millis()
    );

    Ok(table)
}

fn creds() -> (String, String, HashMap<String, String>) {
    let mut hm = HashMap::new();

    let account_key = base64_decode(
        option_env!("GOOGLE_SERVICE_ACCOUNT_KEY").expect("expected GOOGLE_SERVICE_ACCOUNT_KEY"),
    )
    .expect("can't decode the service account key");
    let bucket_name = option_env!("GOOGLE_BUCKET_NAME").expect("expected GOOGLE_BUCKET_NAME");

    hm.insert(
        "GOOGLE_SERVICE_ACCOUNT_KEY".to_string(),
        account_key.clone(),
    );

    hm.insert("GOOGLE_BUCKET_NAME".to_string(), bucket_name.to_string());
    (bucket_name.to_string(), account_key.to_string(), hm)
}

// Decode BASE64_STANDARD encoded string
pub fn base64_decode(v: &str) -> anyhow::Result<String> {
    use base64::prelude::*;
    Ok(String::from_utf8(BASE64_STANDARD.decode(v.as_bytes())?)?)
}

#[cfg(test)]
mod tests {

    fn tables_to_download() -> Vec<String> {
        vec![
            "sales_order_details".to_string(),
            "customer_group".to_string(),
        ]
    }

    #[tokio::test]
    async fn download_table() -> anyhow::Result<()> {
        super::download_table(
            "sq_avg_monthly_sales",
            "data/485a6aad-78c5-418a-9eef-8fc78c0c5f33/6891af61-gc2bD6wHi/delta",
        )
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn download_tables() -> anyhow::Result<()> {
        for table_name in tables_to_download() {
            super::download_table(
                table_name.as_str(),
                "data/485a6aad-78c5-418a-9eef-8fc78c0c5f33/6891af61-gc2bD6wHi/delta",
            )
            .await?;
        }
        Ok(())
    }
}
