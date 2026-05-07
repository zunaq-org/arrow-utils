# Datafusion Logical Plan Filters


## Ask to generate the query by providing the rows and schema

We are working with datafusion and trying to build the logical-plan from a sql and after that 
applying couple of filters on the logical plan, means that user will provide the extra filters 
with the sql which we have to apply to logical plan. You can search about in current datafusion 
repo and check how does logical plan and about its api, and please do not change any code in this 
as it mainly for your help to read the api code.

we might face an issue about if we have different table with nested queries or join how to 
figure-out in the query where to apply the filter 
is the question? by the way you can also explore the codebase in the same repository and check 
about tree_node has to offer for the rewriting the logical plans, main thing is we have to create 
a strategy about api interface to provide the sql and filter/expressions

I am providing you top 50 rows of the table with its schema, so that it can be helpful for you to
generate
the test cases queries, like nested, joins nested to nested queries.


First let me know what can be approaches, what datafusion provides for us to solve the problem. 
Please don't change anything in the project itself, because this project is for
your reference, we are working on a separate project to make it happen.


## Asking Doubt about the multiple join and filter extend

I have a doubt how should we take it into account about join and filter with multiple
table and inner queries, because user will provide only filters and queries? I mean how
should we confirm the location of filter injection


### Answer

✦ That's an excellent and crucial question. You've hit on the core challenge of this task:
ensuring the filters are applied to the correct tables, especially within complex queries,
without having to manually parse the SQL or understand its intent.


The current approach handles this automatically and safely by leveraging the structure
of the `LogicalPlan` tree and the `TreeNodeRewriter` pattern. Here’s a breakdown of how
the location for filter injection is confirmed:


### The Core Concept: Targeting the Source


The fundamental principle is that we don't try to understand the entire complex query
(the joins, the subqueries, etc.). Instead, we focus on the most basic and essential
node in any query plan: the `TableScan`.


1. What is a `LogicalPlan::TableScan`?
   No matter how nested or complex a query is, if it reads data from a table named
   sales, there will be a LogicalPlan::TableScan node in the plan tree that represents
   "read data from the sales table." This is the ultimate source of the data for that
   table.


2. How the `TreeNodeRewriter` Works:
   Our FilterPusher implements the TreeNodeRewriter. This rewriter performs a bottom-up
   (`f_up`) traversal of the entire LogicalPlan tree. This means it visits and processes the
   children of a node before processing the node itself.


3. The Injection Logic:
   The logic inside our f_up implementation is very specific:
    * It checks only one thing: "Is this LogicalPlan node a TableScan?"
    * If it is, it then asks: "Does the table_name in this TableScan node match the
      table_name in any of my ExtraFilters?"
    * If both are true, it injects a new Filter node directly on top of that TableScan
      node, transforming that part of the tree from TableScan to Filter -> TableScan.
    * If the node is not a TableScan (e.g., it's a Join, Aggregate, Projection, etc.), it
      simply ignores it and passes it up the tree unmodified.


How This Solves the Join and Subquery Problem

Let's walk through how this simple rule correctly handles complex cases:


### Scenario 1: A Multi-Table Join


Consider this query from our tests:


1 SELECT s.product_name, c.category_name, i.stock_count
2 FROM sales s
3 JOIN categories c ON s.category_id = c.category_id
4 JOIN inventory i ON s.product_id = i.product_id

And these filters:
* ExtraFilter for sales on list_price
* ExtraFilter for inventory on stock_count

Here's how the bottom-up rewrite works:


1. The rewriter starts at the leaves of the plan tree. It finds three TableScan nodes:
   TableScan(sales), TableScan(categories), and TableScan(inventory).
2. When it visits TableScan(sales), it finds a matching filter and rewrites that part of
   the plan to Filter(sales.list_price) -> TableScan(sales).
3. When it visits TableScan(categories), it finds no matching filter and leaves it as is.
4. When it visits TableScan(inventory), it finds a matching filter and rewrites it to
   Filter(inventory.stock_count) -> TableScan(inventory).
5. Next, it moves up to the Join nodes. Our rewriter's logic ignores Join nodes. It simply
   sees that its children have already been rewritten. The Join is now joining the already
   filtered results.


The location is confirmed because we are not trying to inject the filter at the Join
level. We are "pushing it down" to the source (TableScan) before the join even happens.


### Scenario 2: A Nested Subquery


Consider this query:


1 SELECT product_name FROM sales
2 WHERE category_id IN (SELECT category_id FROM categories WHERE category_name =
'Bikes')

And a filter for the categories table.


1. The LogicalPlan for this query will contain a main query structure and a separate
   subquery structure within it.
2. The TreeNodeRewriter doesn't care that it's a subquery. It traverses all nodes.
3. It will eventually find the TableScan(categories) node inside the subquery plan.
4. It will see that the table name matches the ExtraFilter and inject the filter right
   there, inside the subquery.


Summary: Why This Approach is Robust


* Decoupling: The filter logic is completely decoupled from the query's complexity. It
  doesn't need to know about joins, unions, or subqueries.
* Single Responsibility: The rewriter has one job: find the TableScan for a given table
  name and attach a filter.
* Leveraging the Optimizer: This pattern of "pushing predicates down" is exactly what
  query optimizers do. We are essentially creating a custom optimizer rule. By targeting
  the TableScan, we guarantee the filter is applied at the earliest possible moment,
  which is the most efficient place for it.


So, to directly answer your question: we confirm the location by ignoring the complex
structure and targeting the simple, universal source node (`TableScan`) identified by
its unique table name.

