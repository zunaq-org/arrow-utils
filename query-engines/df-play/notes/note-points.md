# Note Points

- Table Scan filters are easy to implement with the input cols
- Table name can have aliases while applying the filters
  - need to handle them, either by the alias name or something else
  - it logical plan gave the aliases in case of aliases and not the table name
  - problem if in extra filters table-name are coming instead of and in query aliases
- what about the binary operator between the user given filters `OR` and `AND`.
- what about the computed columns in the projection and also in the filter
  - `SELECT a + 1 AS b FROM t WHERE b > 10`
  - issue is that in the table-scan `b` does not exist, so we cannot apply this filter blindly 
    when scanning the table, so these filters can't pushdown
  - we must rewrite b to its defining expression a + 1 to legally move the filter
- what about having filters in the aggregate
  - means aliases that wrap `aggregates/window` functions cannot be pushed down below 
    Aggregate/Window
  - `SELECT SUM(x) AS s FROM t WHERE s > 10`
  - in the above example s is defined an aggregate, rewriting s > 10 to SUM(x) > 10 and pushing 
    below is illegal because it turns a having into where semantics.
- quoted strings may also create some problem because of the cases


## Places to Change The Filter

- TableScan
- SubQueryAliases
- Filters
- etc

- but the problem is how do we know where to apply the extra filters
- means let's first discuss the scope of the project


## Mostly Need to Handle the cases

- handle the subquery aliases first
- handle the projection
- handle the table-scan for base table names
- for join leave it to the optimizers
- aggregate window never push them down

## What can we even achieve

with the help of filter pushdown and late materialization
readers skip irrelevant rows while scanning data from parquet, leveraging Parquet's columnar 
layout by first reading only filter columns, and then selectively reading other columns only for matching rows.