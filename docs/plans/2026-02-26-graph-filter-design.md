# Graph View Filtering — Design

## Goal

Add kind toggles and name search to the resource graph view so users can focus on specific resources while keeping spatial context.

## Approach

UI-only filtering (Approach A). No `kore-core` changes. Filter the already-fetched graph data in the graph view (`crates/kore-app/src/views/graph.rs`).

## UI

- Filter bar in the top-left area, below the node count badge, glass-styled.
- Kind toggle chips: one per kind present in the graph, colored with existing `KIND_COLORS`. All active by default. Click to toggle.
- Name search input: small text field with search icon, substring match (case-insensitive).

## Filtering Logic

1. A node is a **direct match** if its kind is toggled on AND (search is empty OR name contains search string).
2. A node is a **connected match** if directly connected (1 hop via edges) to any direct match.
3. Direct matches: full opacity. Connected matches: ~60% opacity. Non-matches: ~15% opacity.
4. Edges between two visible nodes stay visible; others dim to ~15%.
5. No filters active = everything renders normally.

## Layout

Layout stays fixed regardless of filters — nodes dim but don't move. Spatial orientation is preserved.

## Implementation Scope

Implemented in `crates/kore-app/src/views/graph.rs`: view state holds the kind toggles and search string, visibility is computed per render, the filter bar is drawn above the canvas, and non-matching nodes are dimmed.
