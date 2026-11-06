//! The blocks of a window as a model finds them on its picture: cards, panels, tables, rows,
//! pictures and the groups they stand in, a tree with the window as its root. The program puts
//! the tree in order and decides what lights up under the mouse: that is our own code, and the
//! model knows nothing of it. Coordinates are pixels of the window's picture.

use crate::geometry::{Point, Rect};
use crate::image::Image;

/// How near two blocks the mouse is at their junction, at the scale of 100%.
const NEAR: f32 = 24.0;
/// An only child this much of its parent merges with it.
const MERGE: f32 = 0.9;
/// How much of a frame's side a real edge runs along for the side to go to it.
const EDGE_ALONG: f32 = 0.7;
/// How far neighbouring pixels are apart in a channel across an edge.
const EDGE_STEP: i32 = 12;
/// How many places along a side are looked at.
const EDGE_PLACES: i32 = 64;

/// What the model says of a block. `parent` is the `id` of another block, or 0 for the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub id: u32,
    pub parent: u32,
    pub rect: Rect,
    pub score: f32,
    /// The block lies over others, a menu, a dialog or a popup: the top of a branch that does.
    pub over: bool,
}

/// A block of the tree. Its parent is the place of another block, or nothing for the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    parent: Option<usize>,
    rect: Rect,
    over: bool,
}

/// The tree of a window's blocks, in order.
#[derive(Debug, Default)]
pub struct Tree {
    blocks: Vec<Block>,
    /// Every block's distance from the window.
    depths: Vec<usize>,
    /// The branches that lie over the others, the smallest first.
    branches: Vec<Vec<usize>>,
    /// All the other blocks.
    rest: Vec<usize>,
}

impl Tree {
    /// Puts what the model found in order: blocks less sure than `least` go, and their children
    /// go to their parents; a parent and its only child of nearly its size are one block. A parent
    /// that isn't there, or a block that comes back to itself through its parents, stands in the
    /// window.
    pub fn new(found: &[Found], least: f32) -> Tree {
        let kept: Vec<&Found> = found.iter().filter(|block| block.score >= least).collect();
        let place = |id: u32| kept.iter().position(|block| block.id == id);
        // Up from a block's parent to the nearest one kept: what was dropped is passed through.
        let parent_of = |block: &Found| {
            let mut parent = block.parent;
            let mut steps = 0;
            while parent != 0 && steps <= found.len() {
                if place(parent).is_some() {
                    return place(parent);
                }
                parent = found.iter().find(|other| other.id == parent).map_or(0, |other| other.parent);
                steps += 1;
            }
            None
        };
        let mut blocks: Vec<Block> =
            kept.iter().map(|block| Block { parent: parent_of(block), rect: block.rect, over: block.over }).collect();
        cut_circles(&mut blocks);
        merge_only_children(&mut blocks);
        Tree::of(blocks)
    }

    fn of(blocks: Vec<Block>) -> Tree {
        let mut tree = Tree { blocks, ..Tree::default() };
        tree.depths = (0..tree.blocks.len()).map(|block| tree.chain(block).len() - 1).collect();
        let mut lying: Vec<usize> = (0..tree.blocks.len()).filter(|&block| tree.blocks[block].over).collect();
        lying.sort_by_key(|&block| tree.area(block));
        tree.branches = lying.into_iter().map(|top| tree.branch(top)).collect();
        let taken: Vec<usize> = tree.branches.iter().flatten().copied().collect();
        tree.rest = (0..tree.blocks.len()).filter(|block| !taken.contains(block)).collect();
        tree
    }

    /// The block and its parents, the one in the window last.
    fn chain(&self, block: usize) -> Vec<usize> {
        let mut chain = vec![block];
        while let Some(parent) = self.blocks[chain[chain.len() - 1]].parent {
            chain.push(parent);
        }
        chain
    }

    /// A block and all that is under it.
    fn branch(&self, top: usize) -> Vec<usize> {
        let mut branch = vec![top];
        let mut next = 0;
        while next < branch.len() {
            let upper = branch[next];
            branch.extend((0..self.blocks.len()).filter(|&block| self.blocks[block].parent == Some(upper)));
            next += 1;
        }
        branch
    }

    fn area(&self, block: usize) -> i64 {
        area(self.blocks[block].rect)
    }

    /// Whether the first block is a parent, at any distance, of the second.
    fn above(&self, upper: usize, lower: usize) -> bool {
        upper != lower && self.chain(lower).contains(&upper)
    }

    /// The nearest parent that blocks have in common: nothing for the window.
    fn common(&self, blocks: &[usize]) -> Option<usize> {
        let first = self.chain(*blocks.first()?);
        first.into_iter().skip(1).find(|&upper| blocks[1..].iter().all(|&block| self.above(upper, block)))
    }

    /// Of the blocks of a layer under the point, the deepest, and the smallest of equally deep ones.
    fn deepest(&self, layer: &[usize], p: Point) -> Option<usize> {
        let under = layer.iter().copied().filter(|&block| self.blocks[block].rect.contains(p));
        under.min_by_key(|&block| (std::cmp::Reverse(self.depths[block]), self.area(block)))
    }

    /// What lights up with the mouse at a point, at the monitor's scale: a block, or nothing for
    /// the window. Branches that lie over the others come first, the smallest of them; the mouse
    /// then stays within that layer. On a block, or in the empty place of a group, that block or
    /// group lights up. Outside all of them, near two neighbouring blocks, their nearest common
    /// parent does.
    pub fn lit(&self, p: Point, scale: f32) -> Option<Rect> {
        let on = |block: &usize| self.blocks[*block].rect.contains(p);
        let layer = self.branches.iter().find(|branch| branch.iter().any(on)).unwrap_or(&self.rest);
        let block = self.deepest(layer, p).or_else(|| {
            let near: Vec<usize> =
                layer.iter().copied().filter(|&block| away(self.blocks[block].rect, p) as f32 <= NEAR * scale).collect();
            // Only the lowest count: a group is near because its blocks are.
            let lowest: Vec<usize> =
                near.iter().copied().filter(|&block| !near.iter().any(|&other| self.above(block, other))).collect();
            if lowest.len() >= 2 { self.common(&lowest) } else { None }
        });
        block.map(|block| self.blocks[block].rect)
    }
}

fn area(rect: Rect) -> i64 {
    i64::from(rect.width()) * i64::from(rect.height())
}

/// How far a point is from a box: 0 on it, otherwise the farther of the two axes.
fn away(rect: Rect, p: Point) -> i32 {
    (rect.left - p.x).max(p.x + 1 - rect.right).max(rect.top - p.y).max(p.y + 1 - rect.bottom).max(0)
}

/// A block whose parents lead back to it stands in the window.
fn cut_circles(blocks: &mut [Block]) {
    for block in 0..blocks.len() {
        let mut upper = blocks[block].parent;
        let mut steps = 0;
        while let Some(parent) = upper {
            steps += 1;
            if steps > blocks.len() {
                blocks[block].parent = None;
                break;
            }
            upper = blocks[parent].parent;
        }
    }
}

/// A parent and its only child of nearly its size are one block, with the parent's frame.
fn merge_only_children(blocks: &mut Vec<Block>) {
    loop {
        let only = (0..blocks.len()).find(|&child| {
            let Some(parent) = blocks[child].parent else { return false };
            let kids = blocks.iter().filter(|block| block.parent == Some(parent)).count();
            kids == 1 && area(blocks[child].rect) as f32 >= MERGE * area(blocks[parent].rect) as f32
        });
        let Some(child) = only else { return };
        let (parent, over) = (blocks[child].parent, blocks[child].over);
        if let Some(parent) = parent {
            blocks[parent].over |= over;
        }
        blocks.remove(child);
        for block in blocks.iter_mut() {
            if block.parent == Some(child) {
                block.parent = parent;
            }
            block.parent = block.parent.map(|upper| if upper > child { upper - 1 } else { upper });
        }
    }
}

/// A frame with its sides at the real edges next to them, no further than `reach` pixels away:
/// the model finds frames on a smaller picture, and a line of a pixel or two is lost there. A side
/// with no edge along most of it stays, as around plain text.
pub fn fitted(picture: &Image, frame: Rect, reach: i32) -> Rect {
    let (width, height) = (picture.width as i32, picture.height as i32);
    let across = |start: i32, end: i32| {
        let step = ((end - start) / EDGE_PLACES).max(1);
        (start..end).step_by(step as usize)
    };
    // How much of a side the edge at a row or a column runs along.
    let row = |y: i32| {
        let places = across(frame.left.max(0), frame.right.min(width));
        along(places.map(|x| differs(picture, (x, y - 1), (x, y))))
    };
    let column = |x: i32| {
        let places = across(frame.top.max(0), frame.bottom.min(height));
        along(places.map(|y| differs(picture, (x - 1, y), (x, y))))
    };
    let fitted = Rect::new(
        best(frame.left, reach, width, column),
        best(frame.top, reach, height, row),
        best(frame.right, reach, width, column),
        best(frame.bottom, reach, height, row),
    );
    if fitted.is_empty() { frame } else { fitted }
}

/// The share of places where there is an edge.
fn along(places: impl Iterator<Item = bool>) -> f32 {
    let (mut edge, mut all) = (0, 0);
    for place in places {
        edge += i32::from(place);
        all += 1;
    }
    if all == 0 { 0.0 } else { edge as f32 / all as f32 }
}

/// The strongest edge within reach of a side, the nearest of equally strong ones, or the side
/// itself if none runs along enough of it. The sides of the picture have nothing beyond them.
fn best(side: i32, reach: i32, size: i32, strength: impl Fn(i32) -> f32) -> i32 {
    let places = (side - reach).max(1)..=(side + reach).min(size - 1);
    let strongest = places.map(|place| (place, strength(place))).fold(None, |best: Option<(i32, f32)>, (place, edge)| {
        match best {
            Some((_, most)) if edge < most => best,
            Some((near, most)) if edge == most && (near - side).abs() <= (place - side).abs() => best,
            _ => Some((place, edge)),
        }
    });
    strongest.filter(|(_, edge)| *edge >= EDGE_ALONG).map_or(side, |(place, _)| place)
}

/// Whether two pixels differ enough to have an edge between them.
fn differs(picture: &Image, a: (i32, i32), b: (i32, i32)) -> bool {
    let start = |(x, y): (i32, i32)| y as usize * picture.stride() + x as usize * 4;
    let (a, b) = (&picture.pixels[start(a)..start(a) + 3], &picture.pixels[start(b)..start(b) + 3]);
    a.iter().zip(b).any(|(a, b)| (i32::from(*a) - i32::from(*b)).abs() > EDGE_STEP)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(id: u32, parent: u32, (left, top, right, bottom): (i32, i32, i32, i32)) -> Found {
        Found { id, parent, rect: Rect::new(left, top, right, bottom), score: 1.0, over: false }
    }

    fn lying(id: u32, parent: u32, rect: (i32, i32, i32, i32)) -> Found {
        Found { over: true, ..found(id, parent, rect) }
    }

    /// A column of two cards 12 px apart, the first with a picture in it; beside them a dense
    /// table, and a menu lying over the table and the gap.
    fn window() -> Tree {
        let blocks = [
            found(1, 0, (20, 20, 320, 660)),
            found(2, 1, (20, 20, 320, 330)),
            found(3, 2, (30, 30, 310, 180)),
            found(4, 1, (20, 342, 320, 660)),
            found(5, 0, (400, 20, 900, 145)),
            found(6, 5, (400, 20, 900, 45)),
            found(7, 5, (400, 45, 900, 70)),
            found(8, 5, (400, 70, 900, 95)),
            found(9, 5, (400, 95, 900, 120)),
            found(10, 5, (400, 120, 900, 145)),
            lying(11, 0, (300, 60, 500, 200)),
            found(12, 11, (310, 70, 490, 100)),
        ];
        Tree::new(&blocks, 0.5)
    }

    fn at(x: i32, y: i32) -> Option<(i32, i32, i32, i32)> {
        window().lit(Point::new(x, y), 1.0).map(|rect| (rect.left, rect.top, rect.right, rect.bottom))
    }

    #[test]
    fn on_a_block_the_deepest_block_under_the_mouse_lights_up() {
        assert_eq!(at(100, 100), Some((30, 30, 310, 180)));
        assert_eq!(at(100, 500), Some((20, 342, 320, 660)));
        assert_eq!(at(700, 57), Some((400, 45, 900, 70)));
    }

    #[test]
    fn the_text_of_a_card_lights_up_the_card_and_its_picture_stays_a_block_of_its_own() {
        assert_eq!(at(100, 250), Some((20, 20, 320, 330)));
    }

    #[test]
    fn the_gap_between_two_blocks_is_the_empty_place_of_their_group() {
        assert_eq!(at(100, 336), Some((20, 20, 320, 660)));
    }

    #[test]
    fn a_place_outside_every_block_is_the_window() {
        assert_eq!(at(5, 5), None);
        assert_eq!(at(700, 400), None);
    }

    #[test]
    fn near_two_blocks_of_a_group_outside_it_the_group_lights_up() {
        assert_eq!(at(350, 332), None);
        assert_eq!(at(335, 336), Some((20, 20, 320, 660)));
    }

    #[test]
    fn what_lies_over_the_others_comes_first_whatever_is_under_it() {
        assert_eq!(at(350, 80), Some((310, 70, 490, 100)));
        assert_eq!(at(450, 150), Some((300, 60, 500, 200)));
    }

    #[test]
    fn an_empty_tree_has_only_the_window() {
        assert_eq!(Tree::new(&[], 0.5).lit(Point::new(10, 10), 1.0), None);
    }

    #[test]
    fn the_reach_of_a_junction_grows_with_the_scale() {
        let blocks = [found(1, 0, (0, 0, 200, 100)), found(2, 1, (0, 0, 80, 100)), found(3, 1, (120, 0, 200, 100))];
        let tree = Tree::new(&blocks, 0.5);
        // Outside the group on the right: near one block alone, and then near none.
        assert_eq!(tree.lit(Point::new(210, 50), 1.0), None);
        let wider = [found(1, 0, (0, 0, 200, 100)), found(2, 1, (0, 0, 200, 40)), found(3, 1, (0, 60, 200, 100))];
        let tree = Tree::new(&wider, 0.5);
        assert_eq!(tree.lit(Point::new(240, 50), 1.0), None);
        assert_eq!(tree.lit(Point::new(240, 50), 2.0), Some(Rect::new(0, 0, 200, 100)));
    }

    #[test]
    fn the_children_of_a_block_too_unsure_go_to_its_parent() {
        let blocks = [
            found(1, 0, (0, 0, 400, 400)),
            Found { score: 0.2, ..found(2, 1, (0, 0, 200, 400)) },
            found(3, 2, (10, 10, 100, 100)),
            found(4, 2, (10, 110, 100, 200)),
        ];
        let tree = Tree::new(&blocks, 0.5);
        assert_eq!(tree.blocks.len(), 3);
        assert_eq!(tree.blocks[1].parent, Some(0));
        assert_eq!(tree.blocks[2].parent, Some(0));
    }

    #[test]
    fn a_parent_and_its_only_child_of_nearly_its_size_are_one_block() {
        let blocks = [
            found(1, 0, (0, 0, 100, 100)),
            found(2, 1, (1, 1, 99, 99)),
            found(3, 2, (10, 10, 40, 40)),
            found(4, 2, (50, 50, 90, 90)),
        ];
        let tree = Tree::new(&blocks, 0.5);
        assert_eq!(tree.blocks.len(), 3);
        assert_eq!(tree.blocks[0].rect, Rect::new(0, 0, 100, 100));
        assert_eq!((tree.blocks[1].parent, tree.blocks[2].parent), (Some(0), Some(0)));
    }

    #[test]
    fn a_child_with_a_neighbour_doesnt_merge_however_large() {
        let blocks = [found(1, 0, (0, 0, 100, 100)), found(2, 1, (0, 0, 100, 95)), found(3, 1, (0, 95, 100, 100))];
        assert_eq!(Tree::new(&blocks, 0.5).blocks.len(), 3);
    }

    /// A white picture with a box of grey on it.
    fn picture_with(card: Rect) -> Image {
        let (width, height) = (300, 200);
        let mut pixels = vec![255; (width * height * 4) as usize];
        for y in card.top..card.bottom {
            for x in card.left..card.right {
                let start = ((y * width + x) * 4) as usize;
                pixels[start..start + 3].fill(200);
            }
        }
        Image { width: width as u32, height: height as u32, pixels }
    }

    #[test]
    fn a_frame_goes_to_the_real_edges_next_to_it() {
        let card = Rect::new(40, 30, 200, 120);
        let picture = picture_with(card);
        assert_eq!(fitted(&picture, Rect::new(43, 28, 197, 123), 4), card);
        assert_eq!(fitted(&picture, card, 4), card);
    }

    #[test]
    fn a_side_stays_where_no_edge_is_within_reach() {
        let card = Rect::new(40, 30, 200, 120);
        let picture = picture_with(card);
        assert_eq!(fitted(&picture, Rect::new(30, 20, 210, 130), 4), Rect::new(30, 20, 210, 130));
    }

    #[test]
    fn a_side_stays_where_an_edge_runs_along_only_a_part_of_it() {
        let picture = picture_with(Rect::new(40, 30, 80, 120));
        let frame = Rect::new(40, 28, 250, 120);
        assert_eq!(fitted(&picture, frame, 4).top, 28);
    }

    #[test]
    fn a_frame_on_the_side_of_the_picture_stays_there() {
        let picture = picture_with(Rect::new(40, 30, 200, 120));
        assert_eq!(fitted(&picture, Rect::new(0, 0, 300, 200), 4), Rect::new(0, 0, 300, 200));
    }

    #[test]
    fn a_parent_that_isnt_there_is_the_window_and_a_circle_is_cut() {
        let blocks = [found(1, 9, (0, 0, 100, 100)), found(2, 3, (0, 0, 50, 50)), found(3, 2, (0, 0, 40, 40))];
        let tree = Tree::new(&blocks, 0.5);
        assert_eq!(tree.blocks[0].parent, None);
        assert!(tree.lit(Point::new(10, 10), 1.0).is_some());
        assert!(tree.depths.iter().all(|&depth| depth < 3));
    }
}
