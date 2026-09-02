#!/usr/bin/env bash
#
# Bootstrap GitHub repository metadata: labels and milestones.
#
# Idempotent. Labels are created or updated in place; milestones that already
# exist are left untouched, so re-running never clobbers progress or due dates.
#
#   ./.github/setup-repo.sh
#   ./.github/setup-repo.sh --prune-defaults
#
# --prune-defaults removes the three stock GitHub labels that collide with the
# type: taxonomy (bug, documentation, enhancement). The remaining stock labels
# are triage-useful and are left alone.
#
# Requires the gh CLI authenticated with `repo` scope.
#
# The label vocabulary must stay in sync with docs/CONTRIBUTING.md, since the
# area: labels and the commit scopes are deliberately the same words.

set -euo pipefail

PRUNE_DEFAULTS=0
for arg in "$@"; do
  case "$arg" in
    --prune-defaults) PRUNE_DEFAULTS=1 ;;
    -h|--help) sed -n '2,20p' "$0" | sed 's/^# \?//'; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

command -v gh >/dev/null 2>&1 || { echo "gh is not installed" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "gh is not authenticated: run 'gh auth login'" >&2; exit 1; }

REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
echo "Repository: $REPO"
echo

# ---------------------------------------------------------------- labels ----

label() { # label <name> <color> <description>
  gh label create "$1" --color "$2" --description "$3" --force --repo "$REPO" >/dev/null
  printf '  label     %s\n' "$1"
}

echo "Areas - one per crate, same words as the commit scopes:"
AREA_COLOR="C5DEF5"
label "area:core"      "$AREA_COLOR" "Foundational types, time, jobs, logging, type registry"
label "area:ecs"       "$AREA_COLOR" "World, storage, queries, scheduler"
label "area:platform"  "$AREA_COLOR" "OS, window, input"
label "area:render"    "$AREA_COLOR" "Render graph, pipelines, shaders"
label "area:physics"   "$AREA_COLOR" "Physics backend and adapter"
label "area:audio"     "$AREA_COLOR" "Audio backend and adapter"
label "area:asset"     "$AREA_COLOR" "Asset pipeline, import cache, GUIDs"
label "area:scene"     "$AREA_COLOR" "Scenes, prefabs, serialization, migrations"
label "area:script"    "$AREA_COLOR" "Scripting backend slot (ADR 0007)"
label "area:ui"        "$AREA_COLOR" "Runtime UI"
label "area:devtools"  "$AREA_COLOR" "Diagnostics, profiling, developer tooling"
label "area:app"       "$AREA_COLOR" "Integration, plugins, lifecycle"
label "area:cli"       "$AREA_COLOR" "engine-cli"
label "area:editor"    "$AREA_COLOR" "Editor application"
label "area:launcher"  "$AREA_COLOR" "Launcher application"
label "area:xtask"     "$AREA_COLOR" "Repository automation"
label "area:ci"        "$AREA_COLOR" "Continuous integration and release automation"
label "area:docs"      "$AREA_COLOR" "Documentation and ADRs"

echo
echo "Types:"
label "type:feat"  "0E8A16" "New capability"
label "type:bug"   "D73A4A" "Behaves differently from what it should"
label "type:perf"  "FBCA04" "Performance or iteration time"
label "type:docs"  "0075CA" "Documentation only"
label "type:chore" "CFD3D7" "Build, tooling, dependencies, housekeeping"
label "type:spike" "D4C5F9" "Time-boxed investigation, throwaway result"

echo
echo "Priority:"
label "P0" "B60205" "Blocks the current phase"
label "P1" "E99695" "Planned for the current phase"
label "P2" "F9D0C4" "Desirable, not scheduled"

echo
echo "State:"
label "blocked"   "000000" "Waiting on something outside this issue"
label "needs-adr" "5319E7" "Requires an accepted ADR before work starts"

if [ "$PRUNE_DEFAULTS" -eq 1 ]; then
  echo
  echo "Pruning stock labels that collide with the type: taxonomy:"
  for stock in bug documentation enhancement; do
    if gh label delete "$stock" --yes --repo "$REPO" >/dev/null 2>&1; then
      printf '  deleted   %s\n' "$stock"
    else
      printf '  absent    %s\n' "$stock"
    fi
  done
fi

# ------------------------------------------------------------ milestones ----

echo
echo "Milestones - one per roadmap phase, described by its exit criterion:"

EXISTING="$(gh api "repos/$REPO/milestones?state=all&per_page=100" -q '.[].title' 2>/dev/null || true)"

milestone() { # milestone <title> <description>
  if printf '%s\n' "$EXISTING" | grep -Fxq "$1"; then
    printf '  exists    %s\n' "$1"
    return
  fi
  gh api "repos/$REPO/milestones" -f title="$1" -f description="$2" >/dev/null
  printf '  created   %s\n' "$1"
}

milestone "Phase 0. Foundation" \
  "Workspace, tooling, CI, licenses, ADR process, core and platform skeleton. Exit: an empty window runs and the workspace passes check and test."
milestone "Phase 1. ECS + App Loop" \
  "Archetype and sparse storage, relations, required components, hooks, stages, fixed timestep, type registry. Exit: 1000-entity update benchmark, deterministic test, and a recorded input that replays to an identical world state."
milestone "Phase 2. Render v1" \
  "Render graph, camera, PBR, shadows, skybox, shader variation budget, PSO collection in CI. Exit: primitive scene at a stable 60 FPS on the reference desktop with no first-use pipeline hitch."
milestone "Phase 3. Assets + Scenes" \
  "GUIDs, import cache keyed by input hash, glTF, KTX2, RON, migrations, prefabs, Truth and the change journal. Exit: save/load round-trip, live scene reload, and a three-way merge of divergent scene edits resolved without manual fixing."
milestone "Phase 4. Physics + 3D Platformer" \
  "Rapier adapter, character controller, interpolation, checkpoints, mobile controls, data-driven behaviour. Exit: playable prototype on desktop and Android; tuning and behaviour data reload without rebuilding."
milestone "Phase 5. Editor" \
  "Hierarchy, inspector generated from the type registry, viewport, gizmos, asset browser, journal-based undo, isolated play world. Exit: a level is built, saved, reopened and played with no editor logic in the runtime, proven by the xtask dependency check."
milestone "Phase 6. Launcher + Packaging + Audio" \
  "Project manager, engine version selection, package formats, .pak, Kira adapter. Exit: one command produces runnable packages for the target platforms."
milestone "Phase 7. Stabilization / 0.1.0" \
  "Performance, diagnostics, docs, release automation, regression game, API and schema snapshot checks. Exit: from an empty machine to a playable project in 15 minutes or less."

echo
echo "Done."
echo
echo "Branch protection on master is not scriptable through this token and must"
echo "be set in Settings > Branches: require a pull request, require the 'host'"
echo "and 'android' checks, allow squash merging only, and delete branches on merge."
