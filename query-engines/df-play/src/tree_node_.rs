use datafusion::common::TableReference;
use datafusion::common::tree_node::{Transformed, TreeNodeRewriter};
use datafusion::logical_expr::{BinaryExpr, LogicalPlan, LogicalPlanBuilder, Operator};
use datafusion::prelude::{Expr, col};
use datafusion::scalar::ScalarValue;
use std::sync::Arc;

pub async fn build_plan_by_query(query: &str, tables: Vec<String>) -> anyhow::Result<LogicalPlan> {
    let ctx = crate::build_ctx();

    for table in tables {
        crate::register_parquet_table(&ctx, table.as_str()).await?;
    }
    let df = ctx.sql(query).await?;
    let plan = df.into_unoptimized_plan();

    Ok(plan)
}

pub struct FilterExpr {
    pub table_name: String,
    pub col_name: String,
    pub value: ScalarValue,
    pub operator: Operator,
}

impl FilterExpr {
    pub fn to_expr(&self) -> Expr {
        let left = col(self.col_name.to_string());
        let right = Expr::Literal(self.value.clone(), None);

        Expr::BinaryExpr(BinaryExpr::new(
            Box::new(left),
            self.operator,
            Box::new(right),
        ))
    }
}

pub struct TraversePlanTree {
    exprs: Vec<FilterExpr>,
}

impl TreeNodeRewriter for TraversePlanTree {
    type Node = LogicalPlan;

    fn f_up(&mut self, plan: LogicalPlan) -> datafusion::common::Result<Transformed<Self::Node>> {
        match &plan {
            LogicalPlan::TableScan(table_scan) => {
                let table_name = match &table_scan.table_name {
                    TableReference::Bare { table } => table.to_string(),
                    TableReference::Partial { schema, table } => format!("{}.{}", schema, table),
                    TableReference::Full {
                        catalog,
                        schema,
                        table,
                    } => format!("{}.{}.{}", catalog, schema, table),
                };

                // let table_name = table_scan.table_name.table();

                println!("Scanning table: {}", table_name);

                let applicable_filters: Vec<Expr> = self
                    .exprs
                    .iter()
                    .filter(|e| e.table_name.eq(&table_name))
                    .map(|e| e.to_expr())
                    .collect();

                if applicable_filters.is_empty() {
                    println!("plan has not transformed");
                    return Ok(Transformed::no(plan));
                }

                let filter_expr = applicable_filters
                    .into_iter()
                    .reduce(|acc, f| acc.and(f))
                    .unwrap();

                let new_plan = LogicalPlanBuilder::from(plan.clone())
                    .filter(filter_expr)?
                    .build()?;

                return Ok(Transformed::yes(new_plan));
            }
            // LogicalPlan::SubqueryAlias(sub_query) => {
            //     let table_name = sub_query.alias.to_quoted_string();
            //     println!("reached to the subquery-alias, table-name: {}", table_name);
            // }

            // Alias-targeting: inject filter BELOW the alias (on its input)
            LogicalPlan::SubqueryAlias(sa) => {
                let alias_name = sa.alias.table().to_string();

                let applicable_filters: Vec<Expr> = self
                    .exprs
                    .iter()
                    .filter(|e| e.table_name == alias_name)
                    .map(|e| e.to_expr())
                    .collect();

                if applicable_filters.is_empty() {
                    return Ok(Transformed::no(plan));
                }

                let filter_expr = applicable_filters
                    .into_iter()
                    .reduce(|acc, f| acc.and(f))
                    .unwrap();

                let new_input = LogicalPlanBuilder::from((*sa.input).clone())
                    .filter(filter_expr)?
                    .build()?;

                return Ok(Transformed::yes(LogicalPlan::SubqueryAlias(
                    datafusion::logical_expr::SubqueryAlias::try_new(
                        Arc::new(new_input),
                        sa.alias.clone(),
                    )?,
                )));
            }

            LogicalPlan::Projection(projection) => {
                // need to check the list of exprs: cols and aliases
            }

            LogicalPlan::Filter(filters) => {
                // need to check all the predicates
            }

            LogicalPlan::Join(join) => {

            }

            _ => {}
        };
        Ok(Transformed::no(plan))
    }
}

#[cfg(test)]
mod tests {
    use crate::tree_node_::FilterExpr;
    use datafusion::common::ScalarValue;
    use datafusion::common::tree_node::TreeNode;
    use datafusion::logical_expr::Operator;

    fn filter_expr() -> Vec<FilterExpr> {
        vec![
            // FilterExpr {
            //     table_name: "cg".to_string(),
            //     col_name: "customer_name".to_string(),
            //     value: ScalarValue::from("ak"),
            //     operator: Operator::Eq,
            // },
            FilterExpr {
                table_name: "sales_order_details".to_string(),
                col_name: "productnum".to_string(),
                value: ScalarValue::from("943 POLYNT NEUTRAL BASE GELCOAT DRUM"),
                operator: Operator::Eq,
            },
        ]
    }

    #[tokio::test]
    async fn testing_demo() {
        let query = r#"
            SELECT
                customer_group.customer_name AS "label_customer_group.customer_name",
                CAST(round(sum(coalesce(sales_order_details.totalprice, 0)), 4) AS DOUBLE) AS "label_sales_order_details.totalprice"
             FROM "customer_group"
             JOIN
                sales_order_details
             ON customer_group.account_id = sales_order_details.accountid
             GROUP BY
                customer_group.customer_name
             ORDER BY
                "label_sales_order_details.totalprice" DESC
             LIMIT 50001
        "#;

        //
        // let _query = "SELECT customer_name, account_type from customer_group";

        let logical_plan = super::build_plan_by_query(
            query,
            vec![
                "sales_order_details".to_string(),
                "customer_group".to_string(),
            ],
        )
        .await
        .expect("something went wrong");
        println!("Plan Before: {}", logical_plan);

        println!("traversing the plan");
        let mut traverse = super::TraversePlanTree {
            exprs: filter_expr(),
        };
        let new_plan = logical_plan
            .rewrite(&mut traverse)
            .expect("plan traversal error")
            .data;

        println!("Plan After: {}", new_plan);
    }

    #[tokio::test]
    async fn testing_aliases() {
        let query = r#"
            SELECT
                cg.customer_name AS "label_customer_group.customer_name",
                CAST(round(sum(coalesce(so.totalprice, 0)), 4) AS DOUBLE) AS
                "label_sales_order_details.totalprice"
             FROM "customer_group" as cg
             JOIN
                sales_order_details as so
             ON cg.account_id = so.accountid
             GROUP BY
                cg.customer_name
             ORDER BY
                "label_sales_order_details.totalprice" DESC
             LIMIT 50001
        "#;

        let logical_plan = super::build_plan_by_query(
            query,
            vec![
                "sales_order_details".to_string(),
                "customer_group".to_string(),
            ],
        )
        .await
        .expect("something went wrong");
        println!("Plan Before: {}", logical_plan);

        println!("traversing the plan");
        let mut traverse = super::TraversePlanTree {
            exprs: filter_expr(),
        };
        let new_plan = logical_plan
            .rewrite_with_subqueries(&mut traverse)
            .expect("plan traversal error")
            .data;


        println!("Plan After: {}", new_plan);
    }
}
