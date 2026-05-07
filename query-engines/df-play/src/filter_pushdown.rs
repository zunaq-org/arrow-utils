use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::common::ScalarValue;
use datafusion::common::TableReference;
use datafusion::common::tree_node::{Transformed, TreeNodeRewriter};
use datafusion::error::Result;
use datafusion::logical_expr::logical_plan::builder::LogicalPlanBuilder;
use datafusion::logical_expr::{BinaryExpr, Expr, LogicalPlan, Operator};
use datafusion::prelude::{SessionContext, col};
use std::sync::Arc;

/// Defines a filter to be applied to a LogicalPlan.
#[derive(Clone)]
pub struct ExtraFilter {
    /// The name of the table to apply the filter to.
    pub table_name: String,
    /// The column to filter.
    pub column_name: String,
    /// The comparison operator.
    pub operator: Operator,
    /// The value to compare against.
    pub value: ScalarValue,
}

impl ExtraFilter {
    /// Creates a DataFusion expression from the filter.
    fn to_expr(&self) -> Expr {
        let left = col(self.column_name.clone());
        let right = Expr::Literal(self.value.clone(), None);
        Expr::BinaryExpr(BinaryExpr {
            left: Box::new(left),
            op: self.operator.clone(),
            right: Box::new(right),
        })
    }
}

/// A TreeNodeRewriter that applies filters to TableScan nodes.
struct FilterPusher {
    filters: Vec<ExtraFilter>,
}

impl TreeNodeRewriter for FilterPusher {
    type Node = LogicalPlan;

    fn f_up(&mut self, plan: LogicalPlan) -> Result<Transformed<LogicalPlan>> {
        if let LogicalPlan::TableScan(ts) = &plan {
            let table_name = match &ts.table_name {
                TableReference::Bare { table } => table.to_string(),
                TableReference::Partial { schema, table } => format!("{}.{}", schema, table),
                TableReference::Full {
                    catalog,
                    schema,
                    table,
                } => format!("{}.{}.{}", catalog, schema, table),
            };

            let applicable_filters: Vec<Expr> = self
                .filters
                .iter()
                .filter(|f| f.table_name == table_name)
                .map(|f| f.to_expr())
                .collect();

            if applicable_filters.is_empty() {
                return Ok(Transformed::no(plan));
            }

            let filter_expr = applicable_filters
                .into_iter()
                .reduce(|acc, e| acc.and(e))
                .unwrap();

            let new_plan = LogicalPlanBuilder::from(plan)
                .filter(filter_expr)?
                .build()?;

            Ok(Transformed::yes(new_plan))
        } else {
            Ok(Transformed::no(plan))
        }
    }
}

/// Applies a list of filters to a LogicalPlan created from a SQL query.
pub async fn apply_filters_to_sql(
    ctx: &SessionContext,
    sql: &str,
    filters: Vec<ExtraFilter>,
) -> Result<LogicalPlan> {
    let df = ctx.sql(sql).await?;
    let plan = df.into_unoptimized_plan();

    let mut rewriter = FilterPusher { filters };
    let rewritten_plan = plan.rewrite_with_subqueries(&mut rewriter)?;

    Ok(rewritten_plan.data)
}

// Example Usage
#[tokio::main]
async fn main() -> Result<()> {
    let ctx = SessionContext::new();

    // Register a dummy table so we can create a logical plan.
    let schema = Arc::new(Schema::new(vec![
        Field::new("productnum", DataType::Utf8, false),
        Field::new("avg_monthly_sales", DataType::Float64, false),
        Field::new("lm1_sales", DataType::Float64, false),
        Field::new("lm2_sales", DataType::Float64, false),
        Field::new("lm3_sales", DataType::Float64, false),
    ]));

    let table_source = Arc::new(datafusion::datasource::empty::EmptyTable::new(
        schema.clone(),
    ));
    ctx.register_table("sales", table_source)?;

    // SQL query
    let sql = "SELECT productnum, avg_monthly_sales FROM sales WHERE avg_monthly_sales > 100.0";

    // Extra filters to apply
    let filters = vec![
        ExtraFilter {
            table_name: "sales".to_string(),
            column_name: "lm1_sales".to_string(),
            operator: Operator::Gt,
            value: ScalarValue::from(50.0),
        },
        ExtraFilter {
            table_name: "sales".to_string(),
            column_name: "lm2_sales".to_string(),
            operator: Operator::Gt,
            value: ScalarValue::from(75.0),
        },
    ];

    println!("Original SQL: {}", sql);
    let original_plan = ctx.sql(sql).await?.into_unoptimized_plan();
    println!("Original Plan:\n{}", original_plan.display_indent_schema());

    let new_plan = apply_filters_to_sql(&ctx, sql, filters).await?;

    println!(
        "\nPlan with Additional Filters:\n{}",
        new_plan.display_indent_schema()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use datafusion::datasource::empty::EmptyTable;
    use datafusion::prelude::SessionContext;
    use std::sync::Arc;

    async fn setup_context() -> SessionContext {
        let ctx = SessionContext::new();

        let sales_schema = Arc::new(Schema::new(vec![
            Field::new("product_id", DataType::Int32, false),
            Field::new("product_name", DataType::Utf8, false),
            Field::new("list_price", DataType::Float64, false),
            Field::new("category_id", DataType::Int32, false),
        ]));
        let sales_source = Arc::new(EmptyTable::new(sales_schema));
        ctx.register_table("sales", sales_source).unwrap();

        let categories_schema = Arc::new(Schema::new(vec![
            Field::new("category_id", DataType::Int32, false),
            Field::new("category_name", DataType::Utf8, false),
        ]));
        let categories_source = Arc::new(EmptyTable::new(categories_schema));
        ctx.register_table("categories", categories_source).unwrap();

        let inventory_schema = Arc::new(Schema::new(vec![
            Field::new("product_id", DataType::Int32, false),
            Field::new("stock_count", DataType::Int32, false),
        ]));
        let inventory_source = Arc::new(EmptyTable::new(inventory_schema));
        ctx.register_table("inventory", inventory_source).unwrap();

        ctx
    }

    #[tokio::test]
    async fn test_simple_query() {
        let ctx = setup_context().await;
        let sql = "SELECT product_name, list_price FROM sales WHERE list_price > 50.0";
        let filters = vec![ExtraFilter {
            table_name: "sales".to_string(),
            column_name: "category_id".to_string(),
            operator: Operator::Eq,
            value: ScalarValue::from(1i32),
        }];

        let plan = apply_filters_to_sql(&ctx, sql, filters).await.unwrap();
        let plan_str = format!("{}", plan.display_indent());

        assert!(plan_str.contains("Filter: sales.category_id = Int32(1)"));
        assert!(plan_str.contains("TableScan: sales"));
    }

    #[tokio::test]
    async fn test_join_query() {
        let ctx = setup_context().await;
        let sql = "SELECT s.product_name, c.category_name FROM sales s JOIN categories c ON s.category_id = c.category_id";
        let filters = vec![
            ExtraFilter {
                table_name: "sales".to_string(),
                column_name: "list_price".to_string(),
                operator: Operator::Gt,
                value: ScalarValue::from(100.0),
            },
            ExtraFilter {
                table_name: "categories".to_string(),
                column_name: "category_name".to_string(),
                operator: Operator::NotEq,
                value: ScalarValue::from("Bikes"),
            },
        ];

        let plan = apply_filters_to_sql(&ctx, sql, filters).await.unwrap();
        let plan_str = format!("{}", plan.display_indent());

        assert!(plan_str.contains("Filter: sales.list_price > Float64(100)"));
        assert!(plan_str.contains("Filter: categories.category_name != Utf8(\"Bikes\")"));
    }

    #[tokio::test]
    async fn test_nested_subquery() {
        let ctx = setup_context().await;
        let sql = "SELECT product_name FROM sales WHERE category_id IN (SELECT category_id FROM categories WHERE category_name = 'Components')";
        let filters = vec![
            ExtraFilter {
                table_name: "sales".to_string(),
                column_name: "list_price".to_string(),
                operator: Operator::Lt,
                value: ScalarValue::from(20.0),
            },
            ExtraFilter {
                table_name: "categories".to_string(),
                column_name: "category_id".to_string(),
                operator: Operator::Gt,
                value: ScalarValue::from(5i32),
            },
        ];

        let plan = apply_filters_to_sql(&ctx, sql, filters).await.unwrap();
        let plan_str = format!("{}", plan.display_indent());

        assert!(plan_str.contains("Filter: sales.list_price < Float64(20)"));
        assert!(plan_str.contains("Filter: categories.category_id > Int32(5)"));
    }

    #[tokio::test]
    async fn test_deeply_nested_query() {
        let ctx = setup_context().await;
        let sql = "\n            SELECT product_name FROM sales WHERE product_id IN (
                SELECT product_id FROM inventory WHERE stock_count > 0 AND product_id IN (
                    SELECT product_id FROM sales WHERE category_id = 1
                )
            )
        ";
        let filters = vec![
            ExtraFilter {
                table_name: "sales".to_string(),
                column_name: "list_price".to_string(),
                operator: Operator::Gt,
                value: ScalarValue::from(1000.0),
            },
            ExtraFilter {
                table_name: "inventory".to_string(),
                column_name: "stock_count".to_string(),
                operator: Operator::Lt,
                value: ScalarValue::from(10i32),
            },
        ];

        let plan = apply_filters_to_sql(&ctx, sql, filters).await.unwrap();
        let plan_str = format!("{}", plan.display_indent());

        assert!(plan_str.contains("Filter: sales.list_price > Float64(1000)"));
        assert!(plan_str.contains("Filter: inventory.stock_count < Int32(10)"));
    }

    #[tokio::test]
    async fn test_cte_query() {
        let ctx = setup_context().await;
        let sql = "WITH cheap_sales AS (SELECT * FROM sales WHERE list_price < 50.0) SELECT product_name FROM cheap_sales";
        let filters = vec![ExtraFilter {
            table_name: "sales".to_string(),
            column_name: "category_id".to_string(),
            operator: Operator::Eq,
            value: ScalarValue::from(2i32),
        }];

        let plan = apply_filters_to_sql(&ctx, sql, filters).await.unwrap();
        let plan_str = format!("{}", plan.display_indent());

        assert!(plan_str.contains("Filter: sales.category_id = Int32(2)"));
    }

    #[tokio::test]
    async fn test_window_function_query() {
        let ctx = setup_context().await;
        let sql = "SELECT product_name, ROW_NUMBER() OVER(PARTITION BY category_id ORDER BY list_price DESC) as rn FROM sales";
        let filters = vec![ExtraFilter {
            table_name: "sales".to_string(),
            column_name: "list_price".to_string(),
            operator: Operator::Gt,
            value: ScalarValue::from(10.0),
        }];

        let plan = apply_filters_to_sql(&ctx, sql, filters).await.unwrap();
        let plan_str = format!("{}", plan.display_indent());

        assert!(plan_str.contains("Filter: sales.list_price > Float64(10)"));
        assert!(plan_str.contains("Window"));
    }

    #[tokio::test]
    async fn test_three_table_join_query() {
        let ctx = setup_context().await;
        let sql = "
            SELECT s.product_name, c.category_name, i.stock_count
            FROM sales s
            JOIN categories c ON s.category_id = c.category_id
            JOIN inventory i ON s.product_id = i.product_id
            WHERE s.list_price > 20.0
        ";
        let filters = vec![
            ExtraFilter {
                table_name: "sales".to_string(),
                column_name: "list_price".to_string(),
                operator: Operator::LtEq,
                value: ScalarValue::from(500.0),
            },
            ExtraFilter {
                table_name: "inventory".to_string(),
                column_name: "stock_count".to_string(),
                operator: Operator::Gt,
                value: ScalarValue::from(0i32),
            },
        ];

        let plan = apply_filters_to_sql(&ctx, sql, filters).await.unwrap();
        let plan_str = format!("{}", plan.display_indent());

        assert!(plan_str.contains("Filter: sales.list_price <= Float64(500)"));
        assert!(plan_str.contains("Filter: inventory.stock_count > Int32(0)"));
    }

    #[tokio::test]
    async fn test_union_query() {
        let ctx = setup_context().await;
        let sql = "SELECT product_name, list_price FROM sales WHERE list_price < 10.0 UNION ALL SELECT product_name, list_price FROM sales WHERE list_price > 1000.0";
        let filters = vec![ExtraFilter {
            table_name: "sales".to_string(),
            column_name: "category_id".to_string(),
            operator: Operator::Eq,
            value: ScalarValue::from(3i32),
        }];

        let plan = apply_filters_to_sql(&ctx, sql, filters).await.unwrap();
        let plan_str = format!("{}", plan.display_indent());

        // The filter should be applied to both sides of the UNION
        assert_eq!(
            plan_str
                .matches("Filter: sales.category_id = Int32(3)")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn test_aggregation_query() {
        let ctx = setup_context().await;
        let sql = "SELECT category_id, AVG(list_price) FROM sales GROUP BY category_id";
        let filters = vec![ExtraFilter {
            table_name: "sales".to_string(),
            column_name: "product_id".to_string(),
            operator: Operator::Gt,
            value: ScalarValue::from(100i32),
        }];

        let plan = apply_filters_to_sql(&ctx, sql, filters).await.unwrap();
        let plan_str = format!("{}", plan.display_indent());

        assert!(plan_str.contains("Aggregate: groupBy=[[sales.category_id]]"));
        assert!(plan_str.contains("Filter: sales.product_id > Int32(100)"));
    }
}
