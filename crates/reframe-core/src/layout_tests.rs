use super::*;

fn split_sized(dir: SplitDir, children: Vec<LayoutNode>, sizes: &[f64]) -> LayoutNode {
    LayoutNode::Split(SplitNode {
        id: format!("s-{}", uuid::Uuid::new_v4()),
        dir,
        children,
        sizes: sizes.to_vec(),
    })
}

fn split(dir: SplitDir, children: Vec<LayoutNode>) -> LayoutNode {
    let sizes = vec![100.0 / children.len() as f64; children.len()];
    split_sized(dir, children, &sizes)
}

fn row(children: Vec<LayoutNode>) -> LayoutNode {
    split(SplitDir::Row, children)
}

fn col(children: Vec<LayoutNode>) -> LayoutNode {
    split(SplitDir::Col, children)
}

/// Structure without ids or exact sizes, e.g. row(a, col(b, c)).
fn shape(node: Option<&LayoutNode>) -> String {
    match node {
        None => "∅".into(),
        Some(LayoutNode::Tile(tile)) => tile.session_id.clone(),
        Some(LayoutNode::Group(group)) => format!("group({})", shape(Some(&group.child))),
        Some(LayoutNode::Split(split)) => {
            let children: Vec<String> = split.children.iter().map(|child| shape(Some(child))).collect();
            let dir = match split.dir {
                SplitDir::Row => "row",
                SplitDir::Col => "col",
            };
            format!("{dir}({})", children.join(", "))
        }
    }
}

fn sizes_of(node: Option<&LayoutNode>) -> Vec<f64> {
    match node {
        Some(LayoutNode::Split(split)) => split.sizes.iter().map(|s| (s * 10.0).round() / 10.0).collect(),
        _ => Vec::new(),
    }
}

fn children_of(node: &LayoutNode) -> &[LayoutNode] {
    match node {
        LayoutNode::Split(split) => &split.children,
        _ => &[],
    }
}

fn child_of(node: Option<&LayoutNode>) -> Option<&LayoutNode> {
    match node {
        Some(LayoutNode::Group(group)) => Some(&group.child),
        _ => None,
    }
}

fn target(session_id: &str, side: Side) -> DropTarget {
    DropTarget {
        key: tile_key(session_id),
        side,
    }
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

fn g() -> NewGroup {
    NewGroup {
        id: Some("g".into()),
        name: "G".into(),
        color: "#f00".into(),
    }
}

fn h() -> NewGroup {
    NewGroup {
        id: Some("h".into()),
        name: "H".into(),
        color: "#00f".into(),
    }
}

// ---------- insertTile ----------

#[test]
fn makes_the_first_tile_the_root() {
    assert_eq!(shape(Some(&insert_tile(None, "a", None))), "a");
}

#[test]
fn without_a_target_two_columns_that_fill_in_turn_3_below_2_4_below_1_5_below_3() {
    let mut root = insert_tile(None, "a", None);
    root = insert_tile(Some(&root), "b", None);
    assert_eq!(shape(Some(&root)), "row(a, b)");
    assert_eq!(sizes_of(Some(&root)), [50.0, 50.0]);
    root = insert_tile(Some(&root), "c", None);
    assert_eq!(shape(Some(&root)), "row(a, col(b, c))");
    root = insert_tile(Some(&root), "d", None);
    assert_eq!(shape(Some(&root)), "row(col(a, d), col(b, c))");
    root = insert_tile(Some(&root), "e", None);
    root = insert_tile(Some(&root), "f", None);
    assert_eq!(shape(Some(&root)), "row(col(a, d, f), col(b, c, e))");
    assert_eq!(sizes_of(Some(&root)), [50.0, 50.0]);
    assert_eq!(sizes_of(children_of(&root).get(1)), [33.3, 33.3, 33.3]);
}

#[test]
fn adds_to_the_emptiest_column_of_a_layout_the_user_built_keeping_its_proportions() {
    let three = row(vec![tile("a"), tile("b"), tile("c")]);
    assert_eq!(
        shape(Some(&append_node(Some(&three), tile("x")))),
        "row(a, b, col(c, x))"
    );
    let three_columns = row(vec![
        col(vec![tile("a"), tile("b")]),
        tile("c"),
        col(vec![tile("d"), tile("e")]),
    ]);
    assert_eq!(
        shape(Some(&append_node(Some(&three_columns), tile("x")))),
        "row(col(a, b), col(c, x), col(d, e))"
    );
    let column = col(vec![tile("a"), tile("b")]);
    assert_eq!(shape(Some(&append_node(Some(&column), tile("x")))), "row(col(a, b), x)");
    let resized = append_node(
        Some(&row(vec![
            col(vec![tile("a"), tile("d")]),
            split_sized(SplitDir::Col, vec![tile("b"), tile("c")], &[70.0, 30.0]),
        ])),
        tile("x"),
    );
    assert_eq!(sizes_of(children_of(&resized).get(1)), [46.7, 20.0, 33.3]);
}

#[test]
fn puts_a_new_tile_below_a_group_on_the_right_not_into_it() {
    let root = group_tile(Some(&row(vec![tile("a"), tile("b")])), "b", g());
    assert_eq!(
        shape(Some(&append_node(root.as_ref(), tile("x")))),
        "row(a, col(group(b), x))"
    );
    let single = group_tile(Some(&tile("a")), "a", g());
    assert_eq!(
        shape(Some(&append_node(single.as_ref(), tile("x")))),
        "row(group(a), x)"
    );
}

#[test]
fn places_a_tile_on_each_side_of_a_target() {
    let base = tile("a");
    let insert = |side| shape(Some(&insert_tile(Some(&base), "b", Some(&target("a", side)))));
    assert_eq!(insert(Side::Left), "row(b, a)");
    assert_eq!(insert(Side::Right), "row(a, b)");
    assert_eq!(insert(Side::Top), "col(b, a)");
    assert_eq!(insert(Side::Bottom), "col(a, b)");
}

#[test]
fn splits_only_the_target_slot_and_flattens_same_direction_splits() {
    let root = split_sized(SplitDir::Row, vec![tile("a"), tile("b")], &[50.0, 50.0]);
    let next = insert_tile(Some(&root), "c", Some(&target("b", Side::Right)));
    assert_eq!(shape(Some(&next)), "row(a, b, c)");
    assert_eq!(sizes_of(Some(&next)), [50.0, 25.0, 25.0]);
}

#[test]
fn nests_a_perpendicular_split_inside_the_target_slot() {
    let root = row(vec![tile("a"), tile("b")]);
    let next = insert_tile(Some(&root), "c", Some(&target("b", Side::Bottom)));
    assert_eq!(shape(Some(&next)), "row(a, col(b, c))");
}

#[test]
fn appends_when_the_target_is_missing() {
    let next = insert_tile(Some(&tile("a")), "b", Some(&target("zzz", Side::Top)));
    assert_eq!(shape(Some(&next)), "row(a, b)");
}

// ---------- removeTile ----------

#[test]
fn collapses_a_split_that_is_left_with_one_child() {
    let Removal { root, removed, .. } = remove_tile(Some(&row(vec![tile("a"), tile("b")])), "a");
    assert_eq!(shape(root.as_ref()), "b");
    assert_eq!(removed, Some(tile("a")));
}

#[test]
fn redistributes_the_freed_space() {
    let before = split_sized(
        SplitDir::Row,
        vec![tile("a"), tile("b"), tile("c")],
        &[50.0, 25.0, 25.0],
    );
    let Removal { root, .. } = remove_tile(Some(&before), "c");
    assert_eq!(shape(root.as_ref()), "row(a, b)");
    assert_eq!(sizes_of(root.as_ref()), [66.7, 33.3]);
}

#[test]
fn empties_the_layout_when_the_last_tile_goes() {
    assert_eq!(remove_tile(Some(&tile("a")), "a").root, None);
}

#[test]
fn records_the_neighbor_as_restore_anchor() {
    let before = split_sized(
        SplitDir::Row,
        vec![tile("a"), tile("b"), tile("c")],
        &[50.0, 30.0, 20.0],
    );
    let hint = remove_tile(Some(&before), "b").hint;
    assert_eq!(
        hint,
        Some(RestoreHint {
            anchor: tile_key("a"),
            side: Side::Right,
            ratio: 30.0 / 80.0,
            share: Some(0.3),
        })
    );
    let first = remove_tile(Some(&col(vec![tile("a"), tile("b")])), "a").hint;
    assert_eq!(
        first,
        Some(RestoreHint {
            anchor: tile_key("b"),
            side: Side::Top,
            ratio: 0.5,
            share: Some(0.5),
        })
    );
}

#[test]
fn removes_a_group_that_becomes_empty() {
    let group = LayoutNode::Group(GroupNode {
        id: "g".into(),
        name: "G".into(),
        color: "#f00".into(),
        child: Box::new(tile("b")),
    });
    let root = row(vec![tile("a"), group]);
    assert_eq!(shape(remove_tile(Some(&root), "b").root.as_ref()), "a");
}

// ---------- restoreNode ----------

#[test]
fn puts_a_collapsed_tile_back_where_it_was() {
    let before = row(vec![tile("a"), col(vec![tile("b"), tile("c")])]);
    let Removal { root, removed, hint } = remove_tile(Some(&before), "c");
    assert_eq!(shape(root.as_ref()), "row(a, b)");
    let after = restore_node(root.as_ref(), removed.unwrap(), hint.as_ref());
    assert_eq!(shape(Some(&after)), "row(a, col(b, c))");
}

#[test]
fn keeps_the_remembered_share() {
    let before = split_sized(
        SplitDir::Row,
        vec![tile("a"), tile("b"), tile("c")],
        &[50.0, 30.0, 20.0],
    );
    let Removal { root, removed, hint } = remove_tile(Some(&before), "b");
    let restored = restore_node(root.as_ref(), removed.unwrap(), hint.as_ref());
    let rounded: Vec<f64> = sizes_of(Some(&restored)).into_iter().map(f64::round).collect();
    assert_eq!(rounded, [50.0, 30.0, 20.0]);
}

#[test]
fn appends_when_the_anchor_is_gone() {
    let Removal { root, removed, hint } = remove_tile(Some(&row(vec![tile("a"), tile("b")])), "b");
    let without_anchor = remove_tile(Some(&append_node(root.as_ref(), tile("x"))), "a").root;
    let restored = restore_node(without_anchor.as_ref(), removed.unwrap(), hint.as_ref());
    assert_eq!(shape(Some(&restored)), "row(x, b)");
}

// ---------- moveTile ----------

#[test]
fn moves_a_tile_next_to_another_one() {
    let root = row(vec![tile("a"), tile("b"), tile("c")]);
    let moved = move_tile(Some(&root), "a", &target("c", Side::Bottom));
    assert_eq!(shape(moved.as_ref()), "row(b, col(c, a))");
}

#[test]
fn ignores_a_drop_onto_itself() {
    let root = row(vec![tile("a"), tile("b")]);
    assert_eq!(move_tile(Some(&root), "a", &target("a", Side::Left)), Some(root));
}

#[test]
fn keeps_every_tile_exactly_once() {
    let mut root = Some(row(vec![tile("a"), col(vec![tile("b"), tile("c")]), tile("d")]));
    root = move_tile(root.as_ref(), "b", &target("d", Side::Top));
    root = move_tile(root.as_ref(), "d", &target("a", Side::Left));
    assert_eq!(sorted(leaves(root.as_ref())), ["a", "b", "c", "d"]);
}

// ---------- groups ----------

#[test]
fn frames_a_tile_and_grows_by_adding_tiles_after_its_last_one() {
    let mut root = Some(row(vec![tile("a"), tile("b"), tile("c")]));
    root = group_tile(root.as_ref(), "a", g());
    assert_eq!(shape(root.as_ref()), "row(group(a), b, c)");
    root = add_to_group(root.as_ref(), "c", "g");
    assert_eq!(shape(root.as_ref()), "row(group(row(a, c)), b)");
    assert_eq!(
        group_of(root.as_ref(), &tile_key("c")).map(|group| group.id.as_str()),
        Some("g")
    );
    assert_eq!(group_of(root.as_ref(), &tile_key("b")), None);
    assert_eq!(group_tile(root.as_ref(), "a", h()), root); // already grouped
}

#[test]
fn grows_around_a_neighboring_tile_instead_of_moving_it() {
    let start = split_sized(SplitDir::Col, vec![tile("a"), tile("b")], &[30.0, 70.0]);
    let mut root = group_tile(Some(&start), "b", g());
    root = add_to_group(root.as_ref(), "a", "g");
    assert_eq!(shape(root.as_ref()), "group(col(a, b))");
    assert_eq!(sizes_of(child_of(root.as_ref())), [30.0, 70.0]);

    let start = row(vec![
        tile("c"),
        split_sized(
            SplitDir::Col,
            vec![tile("a"), tile("b"), tile("d")],
            &[20.0, 30.0, 50.0],
        ),
    ]);
    root = group_tile(Some(&start), "a", g());
    root = group_tile(root.as_ref(), "b", h());
    root = add_to_group(root.as_ref(), "b", "g");
    assert_eq!(shape(root.as_ref()), "row(c, col(group(col(a, b)), d))");
    let column = root.as_ref().and_then(|root| children_of(root).get(1));
    assert_eq!(sizes_of(column), [50.0, 50.0]);
    // Inside the frame, a and b keep their heights (20 : 30 of the column).
    let grown = column.and_then(|column| children_of(column).first());
    assert_eq!(sizes_of(child_of(grown)), [40.0, 60.0]);
    let ids: Vec<&str> = groups(root.as_ref()).iter().map(|group| group.id.as_str()).collect();
    assert_eq!(ids, ["g"]);
}

#[test]
fn takes_a_tile_out_next_to_its_group_and_an_empty_group_disappears() {
    let mut root = group_tile(Some(&row(vec![tile("a"), tile("b")])), "a", g());
    root = add_to_group(root.as_ref(), "b", "g");
    root = remove_from_group(root.as_ref(), "b");
    assert_eq!(shape(root.as_ref()), "row(group(a), b)");
    root = remove_from_group(root.as_ref(), "a");
    assert!(groups(root.as_ref()).is_empty());
    assert_eq!(sorted(leaves(root.as_ref())), ["a", "b"]);
}

#[test]
fn dissolves_a_group_into_its_tiles() {
    let mut root = group_tile(Some(&row(vec![tile("a"), tile("b"), tile("c")])), "a", g());
    root = add_to_group(root.as_ref(), "b", "g");
    assert_eq!(shape(dissolve_group(root.as_ref(), "g").as_ref()), "row(a, b, c)");
}

#[test]
fn moves_a_whole_group_and_a_dragged_tile_joins_or_leaves_groups_by_where_it_lands() {
    let mut root = group_tile(Some(&row(vec![tile("a"), tile("b"), tile("c")])), "a", g());
    root = move_node(root.as_ref(), "g", &target("c", Side::Bottom));
    assert_eq!(shape(root.as_ref()), "row(b, col(c, group(a)))");
    root = move_tile(root.as_ref(), "b", &target("a", Side::Right));
    assert_eq!(
        group_of(root.as_ref(), &tile_key("b")).map(|group| group.id.as_str()),
        Some("g")
    );
    root = move_tile(root.as_ref(), "b", &target("c", Side::Left));
    assert_eq!(group_of(root.as_ref(), &tile_key("b")), None);
}

#[test]
fn never_nests_groups_and_ignores_drops_of_a_group_onto_itself() {
    let mut root = group_tile(Some(&row(vec![tile("a"), tile("b")])), "a", g());
    root = group_tile(root.as_ref(), "b", h());
    let moved = move_node(root.as_ref(), "h", &target("a", Side::Bottom));
    assert_eq!(shape(moved.as_ref()), "col(group(a), group(b))");
    assert_eq!(move_node(root.as_ref(), "g", &target("a", Side::Left)), root);
}

// ---------- normalize and sizes ----------

#[test]
fn flattens_nested_same_direction_splits_with_proportional_sizes() {
    let nested = split_sized(
        SplitDir::Row,
        vec![
            tile("a"),
            split_sized(SplitDir::Row, vec![tile("b"), tile("c")], &[50.0, 50.0]),
        ],
        &[50.0, 50.0],
    );
    let flat = normalize(nested);
    assert_eq!(shape(Some(&flat)), "row(a, b, c)");
    assert_eq!(sizes_of(Some(&flat)), [50.0, 25.0, 25.0]);
}

#[test]
fn accepts_only_matching_finite_sizes() {
    let root = row(vec![tile("a"), tile("b")]);
    let id = node_key(&root);
    assert_eq!(
        sizes_of(set_split_sizes(Some(&root), &id, &[1.0, 3.0]).as_ref()),
        [25.0, 75.0]
    );
    assert_eq!(set_split_sizes(Some(&root), &id, &[1.0, 2.0, 3.0]), Some(root.clone()));
    assert_eq!(set_split_sizes(Some(&root), &id, &[f64::NAN, 1.0]), Some(root));
}
