# Reports

Reports is how the project's work has gone over time, at `/<project>/reports`: how much was done
and how long it took, how many agents worked and what they cost, the owner's decisions, the code
that landed and the board's own health, as charts on one time axis. It is for the owner, who
changes a limit, a rule or a skill and wants to see what that did to the speed and the quality of
what lands. Nothing on it can be changed: it is read from what the machine recorded each hour.

![Reports for the last 24 hours, wide](pictures/reports-wide.png)

## What the owner does here

- **Chooses how far back to look.** **Hours**, **Days** or **Weeks**, then a window of that grain.
- **Reads the four figures.** What was done, how long a task took, how many agents worked and how
  many decisions wait, over the window. A press on a figure's name goes to its chart.
- **Reads one period across every chart.** A tap on a column, or the left and right arrow keys in
  a chart, picks a period: every chart then says that period's values in words over its plot.
  Until one is picked, each chart says its newest recorded period.
- **Hides a series.** A press on a name in a chart's legend takes that series off the chart, so
  the smaller ones can be read; a second press brings it back.
- **Looks for what a change did.** A dashed line crosses every chart where the project's settings
  or its agents' instructions changed.
- **Reads a chart as a table.** **Show the table** under a chart lists every period's values.

## What it shows

From the top: the heading with the grain and window chosen, the two rows of buttons that choose
them, the four figures, the changes marked in the window, then the charts under a heading per
group, and under the charts of tokens what the window and its landed tasks cost.

### The window

| Grain | Windows offered | Chosen with the grain |
| --- | --- | --- |
| **Hours** | 24 hours, 2 days, 7 days | 2 days |
| **Days** | 7, 14, 30 or 90 days | 14 days |
| **Weeks** | 4 or 12 weeks | 12 weeks |

A first visit opens on the last 24 hours by the hour. After that the page opens on the grain and
the window this device last chose, for this project. A window ends with the hour it is read in,
and a week starts on a Monday. Every time on the page is in UTC.

### The figures

| Figure | Says | Under it |
| --- | --- | --- |
| **Done** | the tasks done in the window | the average for each hour, day or week |
| **Lead time** | the median time from a task's creation to its close | how many closed, and the time nine in ten of them closed within (**p90**) |
| **Agents at work** | the average number of sessions at work | the most at once |
| **Open decisions** | the cards waiting on the owner at the window's end | how many were answered and how many auto-approved in the window |

A figure with nothing to say shows **–** and why: lead time is kept by the day, so by the hour it
says *kept by the day*.

### The charts

![Reports for the last 14 days, wide](pictures/reports-days-wide.png)

| Group | Chart | Shows, for each period |
| --- | --- | --- |
| **Flow** | **Tasks done** | the tasks done, each counted once, at its last close |
| | **Open work** | the tasks ready, in progress, in review and blocked at the period's end |
| | **Task-hours** | the hours tasks spent in each of those four |
| | **Lead time** | the median time from creation to close and the time nine in ten closed within (**p90**); by the day or the week only |
| | **Waits between hands** | the median wait from review to a reviewer, from changes asked to work resumed and to review again, and from approval to landing |
| **Workflow** | **Time by phase**, **Time by state** | on a project with a workflow of its own: the task-hours in each phase and in each state, the four largest named and the rest as **Other** |
| **Agents** | **Agents at work** | the average sessions at work as orchestrators, executors, reviewers and in other roles, and a line of the sessions counted once each |
| | **Sessions** | the sessions that connected, went stale and were lost |
| | **Machine load** | the peak and the average of the machine's load |
| **Tokens and cost** | **Estimated cost** | what the agents' model calls would cost at list prices, by role |
| | **Tokens** | the tokens those calls took, by role |
| **Decisions and code** | **Decisions** | cards answered and auto-approved, and a line of those still open |
| | **Commits** | commits and merges that reached the default branch |
| | **Lines changed** | lines added, above the axis, and removed, below it |
| **Quality** | **Bounces, reds and reverts** | tasks sent from review back to work, reds filed, reverts, and changes a review asked for |
| | **Time to fix a red** | the median and the **p90** time from a red's filing to its close |
| **Board health** | **Board requests** | requests to the board that were slow or got no answer, and failed passes of its dispatch and sweep |

Only tasks are counted, never their subtasks. Each chart has its title, a sentence saying exactly
what it counts, its legend, the period picked said in words, its plot with the unit on its axis,
and **Show the table**.

### Tokens and cost

Under the two charts of tokens the page sums the window: the estimated dollars and the tokens in
all; then **Cost per landed task**, with how many tasks landed with tokens recorded, the median
cost of one with its subtasks, the ten dearest, each a link to its page, and what the first
rounds cost against the review rounds after them. Every dollar is an estimate at the providers'
list prices, with the day those prices were read: never money spent, as a subscription is not
billed by the token. A figure marked **+ unpriced** leaves out the tokens of a model the price
list does not name, and the page names those models.

## Gaps are not zeros

A period nothing recorded is drawn hatched, and a line breaks across it: it is never drawn as a
zero. The machine's server records each hour as it passes, so an hour it did not run through, or
whose file it could not read, is a gap. The words over a plot say **no data recorded** for a
gap, and how much of a period was recorded when only part was (*18 of 24 h recorded*). Periods
from before the project's first record are left off the axis altogether.

Each source has its own gaps: the tasks, decisions and code come from one record, the agents and
the machine's load from a second, and the board's health from a third. The hour the page is read
in has no agents and no board health yet, as those are written when an hour ends.

## Marked changes

A dashed line crosses every chart at each commit on the default branch, in the window, that
changed the project's settings, its rules or one of its agents' skills. Above the charts the
newest three are listed, each with its period, what it changed and the commit's subject and
short hash; more are folded under **N more changes**. Picking the period a change fell in says it
over every plot as well.

## Hidden series

Each name in a legend is a button. Pressed, its series leaves that chart alone: the plot, the
words over it and the table. The axis then fits the series left, so hiding a large one makes the
others taller. The choice is kept on this device, for this project, in every window, until the
name is pressed again.

## How fresh it is

A figure is up to an hour old: the machine's server sums the project's hour once an hour. An open
page reads its window again every five minutes, and at once when another window is chosen.

## States

| State | What the page shows |
| --- | --- |
| Loading, first visit | *Loading the reports…* over the outline of four figures and two charts |
| Loading, a later visit | at once, the window as this device last kept it, with *Shown from this device, saved 14:32. Checking for changes…* until the server has answered |
| Nothing recorded yet | *No roll-up yet.*, when the first is due, the four figures at **–** and no chart |
| A file could not be read | **N files not read: their hours show as gaps**, which opens to name each file and why |
| The read fails | **The reports could not be read.**, why, and that it is trying again; what was shown stays |
| The read fails over a kept copy | the kept copy, with *Not reachable* in place of *Checking for changes…* |
| Lead time, by the hour | in the chart's place: *The roll-up keeps lead time by the day: switch to Days or Weeks to see it.* |
| A chart below the screen | its outline, drawn as it comes near the screen |

## On a phone

![Reports on a phone](pictures/reports-phone.png)

One chart to a row, where a wide screen has two side by side from 960 points, and the figures two
to a row, four from 760 points. A tap on a column picks the period. With many marked changes,
only the newest three are listed, so the first chart stays on the first screen.

## Edges

- A server a project runs for itself, rather than the machine's shared one, records no hours:
  on it the page shows what the shared server recorded, or *No roll-up yet*.
- A task approved, sent back by its landing and approved again counts once in **Tasks done**, at
  its last close.
- The days of **Tasks done** are UTC days, so a count by the day can differ from one made by the
  machine's own days.
- An hour's tokens are what the board's count gained between two hourly sums: an hour no two sums
  span has none, and is a gap.
- A session with two roles counts in both in **Agents at work**; the line counts it once.
- A link to one chart (`/<project>/reports#chart-cost`) opens the page at that chart, until a
  window is chosen.
- A change that reaches the default branch outside the window is not marked, and a change to a
  guide is never marked.

## Related

- [Reports and monitoring](../analysis/reports-and-monitoring.md#the-day-files): what the hourly
  record holds and how each figure is computed.
- [Agent activity](../AGENT_ACTIVITY.md): what the agents' hours record.
- [Task board](../TASKS.md#what-the-tokens-cost): how tokens are counted and priced.
- [Runs](RUNS.md): the builds and tests behind the work, one by one.
- [Workflow](../WORKFLOW.md): a project's own phases and states.
