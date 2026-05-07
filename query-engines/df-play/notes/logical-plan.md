      Short answer: only rewriting TableScan nodes is not enough if you want this to be correct and complete for joins, aliases, projections, aggregates/windows, and subqueries
        (especially subqueries embedded in expressions). It will work for many simple cases (including many joins when IN/EXISTS get rewritten to joins), but there are important
gaps.

        What your example does well

            * datafusion-examples/examples/filter_pushdown.rs shows a clean TreeNodeRewriter that:
                * visits TableScan and wraps it with a Filter with per-table predicates

                * this effectively applies pre-join filtering per table

                * works when subqueries are planned as normal plan branches (e.g., IN is often rewritten to a semi-join)
            * It’s simple and performant for base-column filters.

        Where TableScan-only rewriting falls short

            * Subqueries inside expressions


                * rewrite() does not traverse subqueries embedded in expressions (IN (SELECT ...), EXISTS, scalar subqueries). Use rewrite_with_subqueries to reach those. See:

                    * LogicalPlan with subquery visitors/rewriters: datafusion/expr/src/logical_plan/tree_node.rs (apply_with_subqueries, rewrite_with_subqueries, map_subqueries)

                * If the planner did not rewrite a subquery to a join, your current rewriter won’t apply filters to its scans.
            * Aliases and projections


                * If a filter refers to columns aliased by a projection or to expressions (e.g., WHERE b > 10 where b is an alias of a + 1), pushing to TableScan requires rewriting
the
predicate through the projection. The optimizer does this via replace_cols_by_name; see optimizer/src/push_down_filter.rs.

                * Your current approach only handles base-table columns; it won’t rewrite across projections.
            * Join semantics and placement


                * Adding a Filter directly above a TableScan changes semantics relative to a WHERE on the joined output, especially for outer joins. The optimizer’s PushDownFilter
        respects preserved-side rules for joins (lr_is_preserved/on_lr_is_preserved) and will split/push predicates safely. A TableScan-only approach bypasses this logic and can
alter
results if the intent was a post-join filter.
* Multi-table predicates


                * Filters referencing multiple tables (e.g., a.productnum = b.productnum AND a.avg_monthly_sales > 200) need splitting and appropriate placement across both sides.
The
optimizer handles splitting and join-aware placement; TableScan-only injection doesn’t.
* Aggregates and windows


                * Predicates on group/window outputs must be above those nodes, not pushed to scans. The optimizer keeps those above Aggregate/Window and pushes only the parts that
        refer to grouping/partition columns. TableScan-only injection won’t handle these cases or will place them incorrectly.
            * Provider pushdown


                * TableProvider pushdown should use TableScan.filters with respect to TableProviderFilterPushDown (Exact/Inexact/Unsupported) to avoid duplicating predicates
        incorrectly. The PushDownFilter rule already does this negotiation; simple wrapping with Filter nodes won’t.

        Recommendations

            * If the semantics are “WHERE on final result,” add filters at the root and run the optimizer


                * Build the LogicalPlan from SQL, add a top-level Filter with the user’s extra predicates (use conjunction), then run at least SimplifyExpressions and
PushDownFilter.

                * This handles joins, aliases/projections, aggregates/windows, unions, and scan pushdown correctly. See optimizer/src/push_down_filter.rs for logic.
            * If your API is per-table filter (pre-join semantics), you can keep the TableScan rewrite, but:


                * Use rewrite_with_subqueries instead of rewrite so that you also reach subqueries embedded in expressions:

                    * datafusion/expr/src/logical_plan/tree_node.rs: rewrite_with_subqueries, map_subqueries

                * Accept that you’re applying pre-join filters, which can differ from WHERE semantics on outer joins. Document this.

                * Optionally run the optimizer after injection anyway so filters can be further simplified and pushed into scan.filters where supported.
            * For best coverage, support both modes


                * WholeQuery mode: insert at root; run optimizer; let it split and push down safely.

                * ByAlias/ByTable mode: insert in a specific subtree (pre-join semantics). For this, match SubqueryAlias or TableScan nodes by alias/table reference and inject below
        that node, then run optimizer.

        Concrete suggestions for your example

            * Switch from plan.rewrite(...) to plan.rewrite_with_subqueries(...) so scans inside expression subqueries are visited when they are not rewritten to joins.
            * Consider running the logical optimizer after injection so:
                * projection alias rewrites can occur if needed

                * predicates get split/pushed safely

                * scan.filter pushdown is attempted

        What you will still miss with TableScan-only approach (even with rewrite_with_subqueries)

            * Filters on aliased or computed columns (require projection-aware rewriting)
            * Multi-table predicates (require splitting and join-aware placement)
            * Correct WHERE semantics for outer joins if you really wanted post-join behavior
            * Predicates on aggregates/window outputs

        Summary

            * TableScan-only rewrite is fine for simple per-table, base-column filters and many join forms, especially if IN/EXISTS become joins. It will miss or misplace filters in
        more complex scenarios.
            * For correctness and completeness, either:
                * Add filters at the root and rely on PushDownFilter, or

                * Use targeted placement plus rewrite_with_subqueries, then also run the optimizer.
            * If you keep the TableScan approach from filter_pushdown.rs, at minimum switch to rewrite_with_subqueries and document the outer-join semantics and alias/projection
        limitations.

You can read the aboce para where I had discussion with you last, and also you can explore the current repo as well to provide the answer....

We are going to have an awesome discussion..




## Continue


Awesome — I dug through the repo you’ve got checked out and tied it back to the points we discussed last time. Here’s what I see and what I’d recommend.

    What your current example already does right

        * datafusion-examples/examples/filter_pushdown.rs is already using rewrite_with_subqueries when injecting per-table filters. That addresses the “subqueries embedded in
    expressions” traversal gap I called out earlier.
        * You’re injecting Filters directly above TableScan, which is the correct low-overhead way to do “pre-join/per-table” filters without touching TableScan.filters (provider
    pushdown negotiation belongs to the optimizer).
        * The tests cover joins, unions, CTEs, windows, and nested subqueries in a pre-join/per-table semantics model.

    Where you will still see “TableScan-only” gaps (confirmed against the repo)

        * Aliases: Your FilterPusher only matches LogicalPlan::TableScan table_name (TableReference). It does not match SubqueryAlias nodes, so you can’t target alias names like s
    or c — only the base table name. Example: in test_join_query you pass filters for “sales” and “categories” (base names), which works. But a filter targeting alias "s" wouldn’t
    be applied with the current code.
        * Projections/aliases: You still only handle base columns, not computed/aliased expressions. If a query projects b := a + 1 and the filter is on b, placing it near TableScan
     requires rewriting the predicate through the projection; that logic lives in the optimizer (replace_col, push_down_filter) and isn’t replicated by this rewriter.
        * Join semantics: Injecting Filter directly above TableScan is pre-join semantics, which differs from SQL WHERE semantics for outer joins. The optimizer’s PushDownFilter
    (lr_is_preserved/on_lr_is_preserved) knows where it is safe to move things — a manual TableScan injection does not.
        * Multi-table predicates: Conjuncts that reference multiple relations need splitting and join-aware placement — you’re not doing that here (by design).
        * Aggregates/windows: Predicates on aggregate/window outputs can’t be pushed to scans; they must stay above those nodes, and only the parts referencing grouping/partition
    columns can be pushed. Your approach can’t do these rewrites.
        * Provider pushdown: The right place to negotiate TableProviderFilterPushDown (Exact/Inexact/Unsupported) is the optimizer’s PushDownFilter rule. Your code wraps with Filter
     nodes but does not run the optimizer, so no provider pushdown negotiation occurs afterward.

    Concrete repo-grounded improvements

        * Add alias-aware placement
            * Also match LogicalPlan::SubqueryAlias in your rewriter and inject the filter below the alias if ExtraFilter targets an alias name.

            * Good place to do it is in f_up:

                * If plan is SubqueryAlias { input, alias, .. } and any filters target alias, rebuild the alias with input wrapped in a
    Filter(LogicalPlanBuilder::from(input).filter(expr).build()?).

            * Keep your existing TableScan match for base table names. That lets you support both alias-based and base-name targeting.
        * Normalize table name matching
            * Right now you turn TableReference into a fully qualified string (catalog.schema.table) and compare to ExtraFilter.table_name.

            * Consider matching in a more flexible way:

                * Allow “bare table” match against the leaf table name (e.g., sales matches catalog.schema.sales).


                * Optionally accept fully qualified names too.


                * Keep alias matching separate as above.
        * Run the optimizer after injection
            * After you’ve injected Filters, run ctx.state().optimize(&rewritten_plan)? to:

                * Split multi-conjuncts and push safe parts down.


                * Do projection-aware predicate rewrites (aliases, computed columns) where possible.


                * Attempt scan.filters pushdown based on provider capabilities.

            * You can do this right in apply_filters_to_sql using ctx.state().optimize. See datafusion/core/src/execution/session_state.rs::optimize.
        * Keep not touching TableScan.filters directly
            * That negotiation belongs to PushDownFilter and TableProviderFilterPushDown. Wrapping with Filter is the right call; let the optimizer decide if it becomes
    TableScan.filters.
        * If you want SQL WHERE-on-final-result semantics
            * Only do this when your “extra predicates” refer to columns actually available at the root schema (including aliases).

            * Two safe approaches:

                * SQL wrapping: SELECT ... FROM (original_sql) q WHERE extra_predicate_on_root_columns. This guarantees post-projection, post-join semantics. You can build this with
     SQL or LogicalPlanBuilder if you rewrite the predicate in terms of the root schema.


                * DataFrame.filter: df.filter(expr_on_root_schema) but only if the expr references output columns (or aliases) — otherwise it will fail schema validation before any
    optimizer rule runs.

            * Do not try to attach a root-level Filter with references to base-table-only columns — that plan is invalid unless you first rewrite the predicate through projections,
    which is nontrivial. The optimizer can push “valid” filters down across projections; it can’t fix an invalid plan.

    Minimal code sketch to add alias support and optimization

        * In your FilterPusher:
            * Add a branch for LogicalPlan::SubqueryAlias that matches filters by alias and injects them below the alias.

            * Keep TableScan branch for base table names.
        * After rewrite_with_subqueries, call ctx.state().optimize.

    Example shape (not editing files, just the gist):

        * In f_up:
            * match LogicalPlan::SubqueryAlias(sa) { if any filters target sa.alias, build new_input = LogicalPlanBuilder::from(sa.input.clone()).filter(conjunction_of_applicable)?
    .build()?; then rebuild SubqueryAlias with new_input }

            * match LogicalPlan::TableScan(ts) { same as you have now, but match table names flexibly (allow bare leaf name to match) }
        * In apply_filters_to_sql:
            * let rewritten = plan.rewrite_with_subqueries(&mut rewriter)?;

            * let optimized = ctx.state().optimize(&rewritten.data)?;

            * Ok(optimized)

    A few gotchas to watch

        * Ambiguous column names: Because you insert below an alias or scan, unqualified col("x") is fine — it resolves within that subtree’s schema. If you switch to root-level
    WHERE semantics, you must reference root-visible columns (including aliases) or rewrite them first.
        * Outer joins: Document that ByTable/ByAlias injection is pre-join semantics and can change results w.r.t a WHERE clause for outer joins.
        * Scalar/Correlated subqueries: rewrite_with_subqueries will let you reach them, but be mindful that some are decorrelated into joins by the analyzer/optimizer while others
    may remain as subqueries; your by-table injection is still fine because you’re walking subqueries as well.
        * Multi-table predicates: leave to the optimizer. If you ever add support yourself, you’ll need to split conjuncts and apply the lr_is_preserved/on_lr_is_preserved logic,
    which is already implemented in optimizer/src/push_down_filter.rs.

    Suggested “modes” to expose

        * WholeQuery mode (post-join WHERE semantics):
            * Only allow extra filters that reference root-visible columns.

            * Implement via SQL wrapping or DataFrame.filter, then run optimizer.
        * ByTable/ByAlias mode (pre-join semantics):
            * Inject under matching SubqueryAlias or TableScan using rewrite_with_subqueries.

            * Then run the optimizer so it can simplify, split, and pushdown to providers where possible.

    Why this will resolve the “TableScan failures”

        * You’ll now visit and inject under SubqueryAlias (fixing “filters targeting aliases don’t get applied”).
        * You’ll run the optimizer post-injection, so provider pushdown, projection-aware rewrites, and safe join-aware placement happen — avoiding duplicated work and incorrect
    placements.
        * You’ll have a clear separation between pre-join and WHERE semantics, so outer join differences are explicit and documented.

    If you want, I can put together a small patch that:

        * Adds alias-aware injection to examples/filter_pushdown.rs
        * Adds optional optimization after injection
        * Makes table name matching resilient to fully-qualified vs bare names

    Let me know which mode you want to prioritize (post-join WHERE vs pre-join/per-table) and whether you’d like me to implement the patch now.


## Continue: Probable Join Semantic Error

Short answer: it’s not join semantics. You’re rewriting an already optimized plan where
projection pushdown has removed productnum from the TableScan output, so the Filter you inject
refers to a column that isn’t in the input schema anymore.

    Why that exact error happens

        * Your test calls into_optimized_plan() before adding your extra filters.
        * DataFusion’s optimizer has already trimmed the scan to only the columns needed by the
    original SQL. From the error:
            * valid_fields for the TableScan are accountid and totalprice

            * productnum is not present anymore (because the query never referenced it)
        * When you wrap that TableScan with Filter(col("productnum") = ...), schema resolution fails
    with FieldNotFound.

    This is independent of join semantics. You’re simply filtering on a column the scan no longer
    produces.

    How to fix it
    Pick one of these approaches:

        1. Inject before optimization, then run the optimizer

        * Build an unoptimized plan, rewrite it, and then call the optimizer so projection pushdown
    can include columns needed by your injected filters.
        * Also use rewrite_with_subqueries so you reach scans under subqueries if you add those
    later.

    Example shape:

        * In build_plan_by_query:
            * let df = ctx.sql(query).await?;

            * let plan = df.into_unoptimized_plan();
        * After you rewrite:
            * let rewritten = plan.rewrite_with_subqueries(&mut traverse)?.data;

            * let optimized = ctx.state().optimize(&rewritten)?;

            * return optimized

        1. If you must rewrite an already optimized plan, expand the TableScan’s projection

        * When you match a LogicalPlan::TableScan, compute the index of productnum in the underlying
    source schema and add it to ts.projection (union with existing).
        * Reconstruct the TableScan with the new projection and updated projected_schema, then insert
     your Filter above it.
        * This is more involved and duplicates logic that the optimizer already handles, so option 1
    is cleaner.

    Extra recommendations

        * Qualify columns in injected filters to avoid ambiguity if the same column name appears on
    both sides of a join:
            * col(format!("{}.{}", self.table_name, self.col_name))

            * Note: This won’t fix the missing-column issue if the scan has already dropped
    productnum; it only improves name resolution.
        * Prefer rewrite_with_subqueries over rewrite so your rewriter also sees scans inside
    IN/EXISTS/scalar subqueries.
        * If you plan to target alias names (s, d, etc.), match SubqueryAlias nodes and inject below
    them; otherwise your current code only matches base table names.

    Minimal changes to your code

        * Change build_plan_by_query to return the unoptimized plan (or add a separate function that
    builds an unoptimized plan).
        * After calling rewrite_with_subqueries in the test, run ctx.state().optimize(&new_plan)
    before printing/running it.

    In other words:

        * The failure is caused by projection pushdown, not joins.
        * Inject filters before optimization (or re-optimize after injection) so the optimizer can
    carry the needed columns through the scans.


## Continue

* Aliases: Your FilterPusher only matches LogicalPlan::TableScan table_name (TableReference). It
  does not match SubqueryAlias nodes, so you can’t target alias names like s
  or c — only the base table name. Example: in test_join_query you pass filters for “sales” and
  “categories” (base names), which works. But a filter targeting alias "s" wouldn’t
  be applied with the current code.

Query: can you give me more examples with queries with the query that we had in the test cases


## Continue

What your code does today

        * You inject new Filter nodes only at:
            * TableScan: when FilterExpr.table_name matches the physical scan’s table name.

            * SubqueryAlias: when FilterExpr.table_name matches the alias; you insert the
    filter on the alias’s input.
        * You never look at Projection nodes and you never rewrite predicates.
        * FilterExpr.to_expr builds predicates over base columns (col("customer_name"), etc.),
     not over computed columns/aliases.
        * Because there is no expression substitution, if a user wanted to filter on a
    computed/aliased expression (e.g., WHERE b > 10 with b := a + 1 in the SELECT), your
    rewriter has no way to push that filter “below” the projection that defines b.

    Why this matters (tie-back to the paragraph)

        * To push a filter on an alias/computed column down to a TableScan (or even just below
     the Projection), you must first rewrite the predicate in terms of the projection’s input
    columns.
            * Example: If Projection defines b := a + 1 and there is a predicate b > 10, you
    need to substitute b with (a + 1) so the predicate becomes a + 1 > 10 before you can
    attach it below the Projection. That’s the “rewrite the predicate through the projection”
    bit.
        * Your current code does not:
            * Inspect Projection nodes for alias definitions,

            * Build an alias map (alias name -> defining expression), or

            * Substitute alias references in the predicate with those defining expressions.
        * That’s what the paragraph meant by “that logic lives in the optimizer (replace_col,
    push_down_filter) and isn’t replicated by this rewriter.”

    What to add if you want to support alias/computed filters

        * Add a Projection arm in TraversePlanTree::f_up that:
            1. Builds an alias map from the projection list:

                * For every Expr::Alias(expr, alias) in the projection, map alias -> expr.

            2. For any filter you intend to place below this Projection, rewrite the filter
    predicate:

                * Replace every column reference that matches an alias with the defining
    expression from the map.


                * This is a pure substitution; you don’t have to algebraically “solve” it (a +
     1 > 10 → a > 9). Just make sure the rewritten predicate refers only to columns in the
    Projection’s input.

            3. Insert the rewritten filter on the Projection’s input and rebuild the
    Projection on top.

    Sketch of how that looks

        * The exact API names can vary by DataFusion version, but the structure is like this:
        * Match Projection


            * Build alias_map: HashMap<String, Expr> from the projection’s expressions

                * If an expression is Expr::Alias(inner, alias), map alias -> inner.clone()

            * For the filters you want to push below this Projection:

                * Rewrite the predicate by substituting col(alias) → alias_map[alias].clone()


                    * Do this with Expr’s rewrite traversal (TreeNodeRewriter/Expr::rewrite)

            * If you rewrote anything, wrap the Projection’s input with a
    Filter(rewritten_predicate) and then rebuild the Projection on top.

    Notes and cautions

        * You don’t need to algebraically simplify the predicate (a + 1 > 10 → a > 9) to make
    pushdown possible; just substituting aliases so the predicate talks only about base
    columns is sufficient to legally move the filter below the Projection. Simplification is a
     bonus and usually handled by optimizer passes like SimplifyExpressions/ConstantFolding.
        * Be careful to only substitute safe aliases. Generally:
            * Substitution is safe for deterministic expressions.

            * If the alias expression references only columns from the Projection’s input, you
     can push the rewritten predicate below that Projection.
        * After you inject filters, consider running the optimizer
    (ctx.state().optimize(&plan)?) so built-in rules can further push filters and simplify
    expressions.
        * Your current SubqueryAlias arm handles alias-named relations (FROM t AS cg).
    Projection is different — it defines new column names (aliases) within the SELECT list.
    That’s the level where computed/aliased columns arise and where you need the alias map and
     substitution.

    Small API sanity checks

        * Depending on your DataFusion version, Expr::Literal usually takes a single
    ScalarValue, not (value, None). Just make sure the signature matches your DataFusion
    version.
        * If you ever need to qualify columns, you can use col("alias.col") or
    Column::from_qualified_name, but since you’re injecting below the alias/projection,
    unqualified column names often resolve fine in that scope.

    In short

        * Your code pushes filters based on base columns and relation aliases
    (TableScan/SubqueryAlias).
        * It can’t push filters that reference computed/aliased columns because it doesn’t
    inspect Projection or rewrite predicates.
        * To support those cases, add a Projection handler that builds an alias map and
    rewrites predicates by substituting alias references with their defining expressions
    before inserting the filter below the Projection. Running the optimizer afterward will
    take care of further pushdown and simplification.

## Continue

can you explain me further about what issue can happen in projection and also suggest me the
solution?


Great question. “Projection” is exactly where aliasing and computed columns are
introduced, and that’s where a naïve filter injector can go wrong. Here’s what can go
wrong and how to fix it cleanly.

    What can go wrong around Projection

        * Filters referencing projection aliases don’t resolve below the projection


            * Example: SELECT a + 1 AS b FROM t WHERE b > 10

            * If you try to push the filter below the Projection without rewriting, you’ll end
     up with a filter on column b on the TableScan — but b doesn’t exist there. You must
    rewrite b to its defining expression a + 1 to legally move the filter.
        * Aliases that wrap aggregates/window functions cannot be pushed below
    Aggregate/Window


            * Example: SELECT SUM(x) AS s FROM t WHERE s > 10

            * s is defined via an aggregate. Rewriting s > 10 to SUM(x) > 10 and pushing below
     the Aggregate is illegal (it turns a HAVING into WHERE). The correct place for that
    filter is above the Aggregate (HAVING semantics).
        * Volatile/non-deterministic expressions can change semantics if duplicated or moved


            * Example: SELECT random() AS r FROM t WHERE r > 0.5

            * If you rewrite r > 0.5 to random() > 0.5 and push it down, you might evaluate
    random() twice (once in the filter and once in the projection), producing different values
     and wrong semantics. Such predicates must not be pushed through the projection unless you
     can guarantee single evaluation or determinism.
        * Name/qualification pitfalls


            * Quoted aliases that look like qualified names
    ("label_sales_order_details.totalprice") are single identifiers, not relation-qualified
    names. Treat them as a single column name when matching and substituting.

            * If a projection renames a column to an existing name, naive substitution can
    shadow the wrong thing. Use the projection’s DFSchema to resolve which names are visible
    at that node.

            * Case sensitivity: quoted identifiers are case-sensitive; unquoted are usually
    normalized. Be consistent when matching alias names.
        * Expression availability


            * When you push a filter below a projection, the rewritten predicate must
    reference only columns available in the projection’s input. If the alias depends on
    columns that don’t exist in the input (shouldn’t happen in a valid plan) or on computed
    values from a later node, you cannot push it.
        * You don’t need algebraic “solving,” but you must substitute


            * You don’t need to transform a + 1 > 10 into a > 9 to push down across a
    projection. Rewriting b > 10 into a + 1 > 10 is sufficient to move the filter below the
    Projection. Further simplification can be left to the optimizer.

    A practical solution outline

        * When you hit a Projection, build an alias map


            * Map alias_name -> defining_expr for every Expr::Alias(def_expr, alias) in that
    Projection.

            * Only consider “safe” alias expressions for pushdown (see below).
        * Rewrite predicates using that alias map


            * For any filter that references those aliases, rewrite the predicate by
    substituting col(alias_name) with its defining_expr.

            * Use DataFusion’s expression rewriter to walk the Expr and replace alias
    references.
        * Safety checks before pushing


            * Don’t push through aliases whose defining expressions:

                * contain AggregateFunction or WindowFunction nodes (keep those filters above
    Aggregate/Window — HAVING),


                * are non-deterministic/volatile (e.g., random(), now(), stateful UDFs).

            * If your DataFusion version exposes a determinism check, use it; otherwise
    implement a conservative walker that rejects known-volatile nodes and UDFs with
    Volatility::Volatile.
        * Rebuild the plan


            * Insert Filter(rewritten_predicate) on the Projection’s input.

            * Rebuild the Projection on top unchanged.

            * After your rewriting pass, let DataFusion’s optimizer run; it can do further
    pushdown and simplification.

    Minimal code sketch (structure)

        * The exact API names vary slightly by DataFusion version, but the shape looks like:
        * In TraversePlanTree::f_up:


            * Match LogicalPlan::Projection(p)

                * Build alias_map: HashMap<String, Expr>


                    * For each e in p.expr:



                        * If Expr::Alias(inner, alias), alias_map.insert(alias.to_string(),
    inner.clone())


                * For each candidate filter you want to insert below this projection:


                    * Check safe_for_pushdown(&alias_map, &predicate) → no aggregates, no
    window, deterministic



                    * Rewrite: predicate.rewrite(AliasSubstituter { alias_map })


                * If any predicate changed:


                    * new_input =
    LogicalPlanBuilder::from((*p.input).clone()).filter(rewritten_predicate)?.build()?



                    * new_plan = Projection(p.expr.clone()) on top of new_input



                    * return Transformed::yes(new_plan)
        * Simple substituter:

    struct AliasSubstituter {
        alias_map: std::collections::HashMap<String, Expr>,
    }

    impl datafusion::common::tree_node::TreeNodeRewriter for AliasSubstituter {
        type Node = Expr;
        fn f_down(&mut self, expr: Expr) -> datafusion::common::Result<Transformed<Expr>> {
            if let Expr::Column(c) = &expr {
                if c.relation.is_none() {
                    if let Some(def) = self.alias_map.get(&c.name) {
                        return Ok(Transformed::yes(def.clone()));
                    }
                }
            }
            Ok(Transformed::no(expr))
        }
    }

        * Safety checks (concept)
            * Walk the defining_expr. If you encounter:

                * Expr::AggregateFunction(_) or Expr::WindowFunction(_): do not push the
    predicate.


                * Non-deterministic/volatile UDFs or builtins: do not push.

            * You can conservatively bail out if unsure; better to miss a pushdown than to
    change semantics.

    How this fits your current code

        * You already handle:
            * TableScan: inject filters for base columns.

            * SubqueryAlias: inject filters under FROM-aliases (cg, so).
        * To support filters on computed/aliased columns in the SELECT list (e.g.,
    "label_sales_order_details.totalprice"):
            * Add the Projection arm above.

            * Extend FilterExpr to support “column alias targets” in addition to table/column
    pairs, or allow table_name to be None for top-level aliases.

                * For quoted aliases like "label_sales_order_details.totalprice", match the
    exact alias string as a single identifier.

            * When you see a matching alias in the projection, rewrite the predicate using the
     alias map and push it below the Projection.

    Gotchas and recommendations

        * Aggregates and HAVING:
            * If the alias wraps an aggregate, the correct place for the predicate is a Filter
     above the Aggregate (HAVING semantics). Do not try to push it to the scan.
        * Window functions:
            * Same story: keep the filter above the Window node; don’t push below.
        * Run the optimizer after your rewrites:
            * After you insert your filters, run DataFusion’s optimizer so rules like
    push_down_filter, simplify_expressions, and prune_columns can do the remaining work.
        * Quoted identifiers with dots:
            * Treat the whole quoted string as the alias name. Use Expr::Column(Column {
    relation: None, name: alias_string }) to compare/match; don’t split on '.'.

    Summary

        * The core projection issue: filters on computed/aliased columns can’t be naively
    pushed down; you must rewrite the predicate through the Projection using an alias map, and
     only push when it’s semantically safe (no aggregates, windows, or volatile functions
    involved).
        * Implement a Projection handler with alias substitution, add safety checks, insert
    the rewritten Filter below the Projection, and let the optimizer finish the job.
