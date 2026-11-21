#!/usr/bin/env bash
# The board the product specs' pictures are taken on (tools/product-pictures.sh, which runs this in
# the fixture project's copy): a few tasks of the made-up recipe site, one at least in every stage
# and waiting on every kind of thing the board names, each written by the board's own commands at
# a moment counted back from NOW, so its thread, its stages and its ages are the same on any day.
#
#   TASKS=PATH NOW=SECONDS tools/product-pictures/board.sh
#
# TASKS is the `tasks` binary and NOW the moment the pictures are taken at, in seconds since 1970.
# A session's handle is drawn at random by the board: this prints `NAME HANDLE` for each session it
# registered, and the caller gives each its NAME in the board it made.
#
# A state the pictures lack is added here: a task, a move or a line, at its own moment.
set -euo pipefail

: "${TASKS:?the tasks binary}" "${NOW:?the moment of the pictures, in seconds}"

hour=3600
day=86400

# Every session registered, by its handle.
sessions=()
# The moment of the last command, counted back from NOW.
last=""

# `ago SECONDS COMMAND…`: a board command run that long before NOW. Each session is seen at that
# moment first (reading its inbox marks it), as an agent at work would be: the board tells a
# command's news to whoever is live, and says so on the thread when nobody is.
ago() {
    local seconds=$1 handle
    shift
    if [ "$seconds" != "$last" ]; then
        last=$seconds
        for handle in ${sessions[@]+"${sessions[@]}"}; do
            TASKS_NOW=$((NOW - seconds)) "$TASKS" inbox --as "$handle" >/dev/null
        done
    fi
    TASKS_NOW=$((NOW - seconds)) "$TASKS" "$@" >/dev/null
}

# `session NAME KIND VARIABLE`: a session registered twelve days before NOW, before any task,
# printed with its name, its handle left in VARIABLE.
session() {
    local handle
    handle="$(TASKS_NOW=$((NOW - 12 * day)) "$TASKS" register --kind "$2")"
    echo "$1 $handle"
    sessions+=("$handle")
    printf -v "$3" '%s' "$handle"
}

lead="" cook="" baker="" taster="" assessor=""
session swift-heron orchestrator lead
session amber-finch worker cook
session lucid-otter worker baker
session keen-wren reviewer taster
session opal-crane worker assessor

# The print view's branch: one commit on the fixture's, made at a moment of its own, so its id is
# the same in every copy.
git checkout --quiet -b site/print-view
mkdir -p site
cat >site/print.css <<'CSS'
@media print {
  header, .comments { display: none; }
  .recipe-photo { float: right; width: 30%; }
  .recipe-steps { font-size: 15pt; }
}
CSS
git add site/print.css
GIT_AUTHOR_DATE="@$((NOW - 7 * hour)) +0000" GIT_COMMITTER_DATE="@$((NOW - 7 * hour)) +0000" \
    git -c user.name=Orchard -c user.email=orchard@example.invalid \
    commit --quiet --message "site/print-view: a recipe prints on one page"
git checkout --quiet main

# In progress, with a subtask done and one going: an agent at work.
ago $((3 * day)) create site/recipe-search "Search recipes by ingredient" --priority 1 --as "$lead" \
    --link docs/engineering/STORAGE.md \
    --body "**What and why.** Readers ask for recipes by what is in the fridge. Today the search reads titles alone.

**Do.** Index every recipe's ingredients and match a search against them, the recipes using most of what was typed first.

**Acceptance.** A search for *leek potato* lists the leek and potato soup first."
ago $((26 * hour)) claim site/recipe-search --as "$cook"
ago $((25 * hour)) create site/search-index "Index every recipe's ingredients" --as "$cook" \
    --parent site/recipe-search --instruction --body "One row an ingredient, by recipe."
ago $((25 * hour)) create site/search-ranking "Rank recipes by how much of the search they use" \
    --as "$cook" --parent site/recipe-search --instruction --body "Most ingredients matched first."
ago $((24 * hour)) claim site/search-index --as "$cook"
ago $((9 * hour)) "done" site/search-index "212 recipes indexed, 1,840 ingredient rows" --as "$cook"
ago $((8 * hour)) claim site/search-ranking --as "$cook"
ago $((8 * hour)) say site/recipe-search "The index is in: 212 recipes, 1,840 ingredient rows. Ranking next." \
    --to @orchestrator --as "$cook"

# In review a second time, its reviewer reading it: the task the task page's pictures show.
ago $((2 * day)) create site/print-view "Print a recipe on one page" --priority 2 --as "$lead" \
    --link docs/DECISIONS.md#recipe-print-view \
    --body "**What and why.** A printed recipe runs to three pages today: the header, the comments and a full-width photo come with it. The owner approved one page.

**Do.** A print style sheet: the photo small beside the title, the steps large, no header, no comments.

**Acceptance.** The lemon tart prints on one A4 page. The owner sees the printed page before it lands.

**Gates.** The page's tests, pictures of the printed page, a fresh reviewer."
ago $((30 * hour)) claim site/print-view --as "$baker"
ago $((22 * hour)) move site/print-view review --as "$baker" \
    --reason "A print style sheet; the lemon tart prints on one page."
ago $((21 * hour)) role site/print-view reviewer --as "$taster"
ago $((19 * hour)) verdict site/print-view changes "The steps print at the screen's size. The brief asks for them large: 14 pt at least." --as "$taster"
ago $((6 * hour)) move site/print-view review --as "$baker" \
    --reason "The steps print at 15 pt and the tart still fits one page."
# What it needs to land, read by an agent that neither filed nor wrote it.
ago $((6 * hour)) needs site/print-view review pictures owner --as "$assessor" \
    --tip "$(git rev-parse refs/heads/site/print-view)" \
    --why "tests: a style sheet alone, which no test reads"
ago $((5 * hour)) role site/print-view reviewer --as "$taster"
ago $((5 * hour)) say site/print-view "Reading the second round now." --as "$taster"

# Blocked in design on a card the owner has not answered.
ago $((4 * day)) create site/search-empty-state "Say something when a search finds nothing" \
    --priority 2 --stage design --as "$lead" --body "A search with no result shows a blank page."
ago $((50 * hour)) claim site/search-empty-state --as "$cook"
ago $((49 * hour)) move site/search-empty-state blocked --on decision:search-empty-state \
    --why "which three recipes to suggest is the owner's choice" --as "$cook"

# Blocked in progress on a check someone has to give.
ago $((5 * day)) create ops/photo-backup "Back up the recipe photos every night" --priority 1 \
    --as "$lead" --body "The photos are on one disk."
ago $((4 * day)) claim ops/photo-backup --as "$baker"
ago $((3 * day)) move ops/photo-backup blocked --on check:restore-drill \
    --why "a restore has not been tried on a copy yet" --as "$baker"

# Ready, behind a task that is still open.
ago $((2 * day)) create site/share-link "Share a search by its link" --priority 3 --as "$lead" \
    --depends site/recipe-search --body "The search's words go in the address."

# Ready with nothing missing, and one more idea in concept: queued.
ago $((7 * hour)) create site/unit-switch "Switch a recipe between cups and grams" --priority 2 \
    --as "$lead" --body "One switch on the recipe's page, kept for the next visit."
ago $((6 * day)) create site/seasonal-menu "Suggest a menu for the season" --priority 3 \
    --stage concept --as "$lead" --body "What is in season, by month."

# Put aside on purpose.
ago $((9 * day)) create site/meal-planner "Plan a week of meals" --priority 4 --as "$lead" \
    --body "Seven dinners from the reader's favourites."
ago $((8 * day)) move site/meal-planner blocked --on parked \
    --why "after the search work, by the owner's order of things" --as "$lead"

# In review with no reviewer: waiting on nobody.
ago $((3 * day)) create docs/style-guide "Write the recipe style guide" --priority 3 --as "$lead" \
    --body "How a recipe is written: quantities, order, tone."
ago $((2 * day)) claim docs/style-guide --as "$cook"
ago $((28 * hour)) move docs/style-guide review --as "$cook" --reason "The guide, with two recipes rewritten by it."

# Put aside for later.
ago $((12 * day)) create site/dark-pages "A dark theme for the recipe pages" --priority 4 --backlog \
    --as "$lead" --body "For reading in a dim kitchen."

# Finished today: one landed after its review, one cancelled.
ago $((2 * day)) create site/favourites "Mark a recipe as a favourite" --priority 2 --as "$lead" \
    --body "A heart on each recipe, and a page of the reader's own."
ago $((40 * hour)) claim site/favourites --as "$cook"
ago $((12 * hour)) move site/favourites review --as "$cook" --reason "The heart and the page."
ago $((11 * hour)) role site/favourites reviewer --as "$taster"
ago $((4 * hour)) verdict site/favourites approve "Read the whole branch: the heart toggles, the page lists them newest first." --as "$taster"
ago $((5 * day)) create site/rss-feed "An RSS feed of new recipes" --priority 4 --as "$lead" \
    --body "For readers who still use a feed reader."
ago $((3 * hour)) move site/rss-feed cancelled --as "$lead" --reason "Nobody asked for it twice."

# Every session at work as the pictures are taken.
ago 0 list
