//! Our model of the blocks of a window, which runs on the computer with ONNX Runtime. Two graphs
//! make it. The first looks at the picture of the window, reduced to a working size, and gives
//! candidate frames, best first, with what it saw at each. The program takes the frames worth
//! going on with, leaving out near copies alone, and the second graph, a small graph network,
//! gives each of them its parent, its final score and whether it lies over others. The program
//! brings the frames back to the window's pixels and puts the tree in order. The numbers of a
//! version of the model are in its `model.json`.

use std::fs;
use std::path::Path;
use std::thread::available_parallelism;

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use serde::Deserialize;

use crate::blocks::{Found, Tree, fitted};
use crate::geometry::Rect;
use crate::image::Image;
use crate::runtime;

/// The most threads a graph runs on.
const THREADS: usize = 8;
/// A narrower or lower box is no frame, in pixels of the reduced picture.
const LEAST: f32 = 2.0;
/// How far a frame's side may be off a real edge: this share of the frame's width or height, at
/// least and at most these many pixels of the reduced picture. The model is off in proportion to
/// a frame, and a long reach would take a small frame to its neighbour's edge.
const OFF: f32 = 0.05;
const OFF_LEAST: f32 = 3.0;
const OFF_MOST: f32 = 24.0;

/// A version of the model, as its `model.json` describes it.
#[derive(Deserialize)]
struct Description {
    files: Files,
    input: Input,
    nodes: Nodes,
    /// The final score below which a frame goes.
    score: f32,
    /// How sure the model is to be that a frame lies over others.
    over: f32,
}

#[derive(Deserialize)]
struct Files {
    frames: String,
    tree: String,
}

#[derive(Deserialize)]
struct Input {
    long_side: u32,
    multiple: u32,
    /// Smaller windows aren't looked at, across and down.
    smallest_window: (u32, u32),
}

#[derive(Deserialize)]
struct Nodes {
    min_score: f32,
    nms_iou: f32,
    max_nodes: usize,
}

/// The model, loaded and ready.
pub struct Finder {
    frames: Session,
    tree: Session,
    description: Description,
}

impl Finder {
    /// Loads the model from its folder, with ONNX Runtime from its library.
    pub fn load(library: &Path, folder: &Path) -> Result<Finder, String> {
        let text = fs::read_to_string(folder.join("model.json")).map_err(|error| error.to_string())?;
        let description: Description = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        let environment = runtime::environment(library)?;
        let threads = available_parallelism().map_or(THREADS, usize::from).min(THREADS);
        // The threads of the two graphs don't wait on the processor: spinning, they get in each
        // other's way, and a window takes more than twice as long.
        let session = |file: &str| {
            Session::builder(&environment)
                .and_then(|builder| builder.with_optimization_level(GraphOptimizationLevel::Level3).map_err(Into::into))
                .and_then(|builder| builder.with_intra_threads(threads).map_err(Into::into))
                .and_then(|builder| builder.with_inter_threads(1).map_err(Into::into))
                .and_then(|builder| builder.with_intra_op_spinning(false).map_err(Into::into))
                .and_then(|mut builder| builder.commit_from_file(folder.join(file)))
                .map_err(|error| error.to_string())
        };
        let (frames, tree) = (session(&description.files.frames)?, session(&description.files.tree)?);
        Ok(Finder { frames, tree, description })
    }

    /// The tree of the blocks that the model finds on a window's picture.
    pub fn find(&mut self, picture: &Image) -> Result<Tree, String> {
        let (mut found, ratio) = self.found(picture)?;
        let least = self.description.score;
        for block in found.iter_mut().filter(|block| block.score >= least) {
            let (across, down) = (reach(block.rect.width(), ratio), reach(block.rect.height(), ratio));
            block.rect = fitted(picture, block.rect, across, down);
        }
        Ok(Tree::new(&found, least))
    }

    /// What the model says of a window's picture, in pixels of the window, and how far the
    /// picture was reduced for it.
    fn found(&mut self, picture: &Image) -> Result<(Vec<Found>, f32), String> {
        let Description { input, nodes, over, .. } = &self.description;
        let (least_wide, least_high) = input.smallest_window;
        if picture.width < least_wide || picture.height < least_high {
            return Ok((Vec::new(), 1.0));
        }
        let ratio = (input.long_side as f32 / picture.width.max(picture.height) as f32).min(1.0);
        let wide = ((picture.width as f32 * ratio + 0.5).floor() as usize).max(1);
        let high = ((picture.height as f32 * ratio + 0.5).floor() as usize).max(1);
        let multiple = input.multiple as usize;
        let (across, down) = (wide.next_multiple_of(multiple), high.next_multiple_of(multiple));
        let planes = planes(picture, (wide, high), (across, down));
        let tensor = Tensor::from_array(([1usize, 3, down, across], planes)).map_err(|error| error.to_string())?;
        let outputs = self.frames.run(ort::inputs!["image" => tensor]).map_err(|error| error.to_string())?;
        let (_, boxes) = outputs["boxes"].try_extract_tensor::<f32>().map_err(|error| error.to_string())?;
        let (_, scores) = outputs["scores"].try_extract_tensor::<f32>().map_err(|error| error.to_string())?;
        let (shape, seen) = outputs["features"].try_extract_tensor::<f32>().map_err(|error| error.to_string())?;
        let length = shape[1] as usize;
        let chosen = chosen(boxes, scores, (wide as f32, high as f32), nodes);
        if chosen.is_empty() {
            return Ok((Vec::new(), ratio));
        }
        let count = chosen.len();
        let features: Vec<f32> = chosen.iter().flat_map(|(row, _)| seen[row * length..][..length].iter().copied()).collect();
        let places: Vec<f32> = chosen.iter().flat_map(|(_, frame)| *frame).collect();
        let inputs = (
            Tensor::from_array(([count, 4usize], places)).map_err(|error| error.to_string())?,
            Tensor::from_array(([count, length], features)).map_err(|error| error.to_string())?,
            Tensor::from_array(([2usize], vec![wide as f32, high as f32])).map_err(|error| error.to_string())?,
        );
        let outputs = self
            .tree
            .run(ort::inputs!["boxes" => inputs.0, "features" => inputs.1, "size" => inputs.2])
            .map_err(|error| error.to_string())?;
        let (_, parents) = outputs["parent"].try_extract_tensor::<i64>().map_err(|error| error.to_string())?;
        let (_, final_scores) = outputs["score"].try_extract_tensor::<f32>().map_err(|error| error.to_string())?;
        let (_, lying) = outputs["over"].try_extract_tensor::<f32>().map_err(|error| error.to_string())?;
        let back = |value: f32, size: u32| ((value / ratio + 0.5).floor() as i32).clamp(0, size as i32);
        let found = chosen.iter().enumerate().map(|(node, (_, [left, top, right, bottom]))| Found {
            id: node as u32 + 1,
            parent: (parents[node] + 1) as u32,
            rect: Rect::new(
                back(*left, picture.width),
                back(*top, picture.height),
                back(*right, picture.width),
                back(*bottom, picture.height),
            ),
            score: final_scores[node],
            over: lying[node] >= *over,
        });
        Ok((found.collect(), ratio))
    }
}

/// How far the sides at either end of a frame's width or height may go to a real edge, in pixels
/// of the window.
fn reach(size: i32, ratio: f32) -> i32 {
    ((size as f32 * ratio * OFF).clamp(OFF_LEAST, OFF_MOST) / ratio).ceil() as i32
}

/// The picture as the first graph takes it, in planes of red, green and blue, top row first:
/// `reduced` is how large it is to be, by the average of the area under each new pixel, and
/// `whole` how large the tensor is, what is left of it staying 0.
fn planes(picture: &Image, reduced: (usize, usize), whole: (usize, usize)) -> Vec<u8> {
    let ((wide, high), (across, down)) = (reduced, whole);
    let (old_wide, old_high) = (picture.width as usize, picture.height as usize);
    let mut planes = vec![0; 3 * across * down];
    // A plane's place of a pixel.
    let at = |plane: usize, x: usize, y: usize| plane * across * down + y * across + x;
    if (wide, high) == (old_wide, old_high) {
        for (y, row) in picture.pixels.chunks_exact(picture.stride()).enumerate() {
            for (x, pixel) in row.chunks_exact(4).enumerate() {
                for plane in 0..3 {
                    planes[at(plane, x, y)] = pixel[2 - plane];
                }
            }
        }
        return planes;
    }
    let (columns, rows) = (shares(old_wide, wide), shares(old_high, high));
    let mut sums = vec![0.0f32; 3 * wide * high];
    let mut line = vec![0.0f32; 3 * wide];
    let mut next = 0;
    for (y, row) in picture.pixels.chunks_exact(picture.stride()).enumerate() {
        line.fill(0.0);
        for share in &columns {
            let pixel = &row[share.old * 4..][..3];
            for plane in 0..3 {
                line[share.new * 3 + plane] += share.part * f32::from(pixel[2 - plane]);
            }
        }
        while next < rows.len() && rows[next].old == y {
            let share = &rows[next];
            for (sum, value) in sums[share.new * 3 * wide..][..3 * wide].iter_mut().zip(&line) {
                *sum += share.part * value;
            }
            next += 1;
        }
    }
    // Rounded as the model's training rounded: half up by a whole ratio, half to even by any other.
    let whole = old_wide.is_multiple_of(wide) && old_high.is_multiple_of(high);
    let rounded = |sum: f32| if whole { (sum + 0.5).floor() } else { sum.round_ties_even() };
    for y in 0..high {
        for x in 0..wide {
            for plane in 0..3 {
                planes[at(plane, x, y)] = rounded(sums[(y * wide + x) * 3 + plane]) as u8;
            }
        }
    }
    planes
}

/// How much of a new pixel an old one makes up, along one side.
struct Share {
    new: usize,
    old: usize,
    part: f32,
}

/// The shares of the old pixels in the new ones when a side is reduced, by new and then old pixel.
fn shares(old: usize, new: usize) -> Vec<Share> {
    let scale = old as f64 / new as f64;
    let mut shares = Vec::with_capacity(old + new);
    for pixel in 0..new {
        let (start, end) = (pixel as f64 * scale, (pixel + 1) as f64 * scale);
        for under in start.floor() as usize..(end.ceil() as usize).min(old) {
            let covered = end.min((under + 1) as f64) - start.max(under as f64);
            if covered > 1e-9 {
                shares.push(Share { new: pixel, old: under, part: (covered / scale) as f32 });
            }
        }
    }
    shares
}

/// Which candidates go on, with their frames kept within the picture: they come best first, and
/// a near copy of a better one stays out. Frames inside one another are kept: many children
/// overlap their parents a good deal, and only the second graph tells a copy from a child.
fn chosen(boxes: &[f32], scores: &[f32], (wide, high): (f32, f32), nodes: &Nodes) -> Vec<(usize, [f32; 4])> {
    let mut chosen: Vec<(usize, [f32; 4])> = Vec::new();
    for (row, (place, score)) in boxes.chunks_exact(4).zip(scores).enumerate() {
        if *score < nodes.min_score {
            break;
        }
        let frame = [place[0].clamp(0.0, wide), place[1].clamp(0.0, high), place[2].clamp(0.0, wide), place[3].clamp(0.0, high)];
        if frame[2] - frame[0] < LEAST || frame[3] - frame[1] < LEAST {
            continue;
        }
        if chosen.iter().any(|(_, kept)| overlap(&frame, kept) >= nodes.nms_iou) {
            continue;
        }
        chosen.push((row, frame));
        if chosen.len() == nodes.max_nodes {
            break;
        }
    }
    chosen
}

/// The area two frames share over the area they cover together.
fn overlap(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let across = a[2].min(b[2]) - a[0].max(b[0]);
    let down = a[3].min(b[3]) - a[1].max(b[1]);
    if across <= 0.0 || down <= 0.0 {
        return 0.0;
    }
    let common = across * down;
    common / ((a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - common)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;

    fn picture(width: u32, height: u32, color: impl Fn(u32, u32) -> [u8; 3]) -> Image {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let [red, green, blue] = color(x, y);
                pixels.extend_from_slice(&[blue, green, red, 255]);
            }
        }
        Image { width, height, pixels }
    }

    #[test]
    fn a_picture_goes_into_planes_of_red_green_and_blue_with_the_rest_at_zero() {
        let shot = picture(2, 1, |x, _| if x == 0 { [10, 20, 30] } else { [40, 50, 60] });
        let planes = planes(&shot, (2, 1), (3, 2));
        assert_eq!(planes.len(), 18);
        assert_eq!(&planes[..3], &[10, 40, 0]);
        assert_eq!(&planes[6..9], &[20, 50, 0]);
        assert_eq!(&planes[12..15], &[30, 60, 0]);
        assert!(planes[3..6].iter().all(|&value| value == 0));
    }

    #[test]
    fn a_picture_is_reduced_by_the_average_of_the_area_under_each_pixel() {
        let shot = picture(4, 2, |x, _| [if x < 2 { 0 } else { 100 }, 200, 50]);
        let planes = planes(&shot, (2, 1), (2, 1));
        assert_eq!(planes, vec![0, 100, 200, 200, 50, 50]);
    }

    #[test]
    fn a_new_pixel_takes_a_part_of_the_old_ones_it_covers() {
        let shot = picture(3, 1, |x, _| [[0, 90, 180][x as usize], 0, 0]);
        let planes = planes(&shot, (2, 1), (2, 1));
        assert_eq!(&planes[..2], &[30, 150]);
    }

    #[test]
    fn the_shares_of_every_new_pixel_add_up_to_it() {
        for (old, new) in [(3840, 1280), (1366, 1280), (1921, 1280), (7, 3)] {
            let mut sums = vec![0.0; new];
            for share in shares(old, new) {
                sums[share.new] += share.part;
            }
            assert!(sums.iter().all(|sum| (sum - 1.0f32).abs() < 1e-4), "{old} to {new}");
        }
    }

    #[test]
    fn a_frames_reach_grows_with_it_between_its_limits() {
        assert_eq!(reach(40, 1.0), 3);
        assert_eq!(reach(200, 1.0), 10);
        assert_eq!(reach(800, 1.0), 24);
        // A window reduced to two thirds: the same in pixels of the reduced picture.
        assert_eq!(reach(300, 2.0 / 3.0), 15);
        assert_eq!(reach(1500, 2.0 / 3.0), 36);
    }

    fn nodes() -> Nodes {
        Nodes { min_score: 0.1, nms_iou: 0.8, max_nodes: 3 }
    }

    #[test]
    fn near_copies_stay_out_and_frames_inside_one_another_stay_in() {
        let boxes = [0., 0., 100., 100., 1., 1., 99., 99., 10., 10., 90., 60.];
        let found = chosen(&boxes, &[0.9, 0.8, 0.7], (200.0, 200.0), &nodes());
        assert_eq!(found.iter().map(|(row, _)| *row).collect::<Vec<_>>(), [0, 2]);
    }

    #[test]
    fn candidates_end_at_the_least_score_and_the_most_nodes() {
        let boxes = [0., 0., 10., 10., 20., 0., 30., 10., 40., 0., 50., 10., 60., 0., 70., 10.];
        assert_eq!(chosen(&boxes[..12], &[0.9, 0.2, 0.05], (100.0, 100.0), &nodes()).len(), 2);
        assert_eq!(chosen(&boxes, &[0.9, 0.8, 0.7, 0.6], (100.0, 100.0), &nodes()).len(), 3);
    }

    #[test]
    fn a_frame_is_kept_within_the_picture_and_too_thin_a_one_goes() {
        let boxes = [-5., -5., 500., 50., 10., 10., 11., 90.];
        let found = chosen(&boxes, &[0.9, 0.8], (100.0, 100.0), &nodes());
        assert_eq!(found, vec![(0, [0., 0., 100., 50.])]);
    }

    #[test]
    fn a_description_is_read_as_model_json_has_it() {
        let text = r#"{
          "contract": 4, "version": "0.1", "files": { "frames": "areas-frames.onnx", "tree": "areas-tree.onnx" },
          "input": { "long_side": 1280, "resize": "area", "multiple": 32, "pad": 0, "type": "uint8", "smallest_window": [320, 200] },
          "frames": { "candidates": 1024, "features": 53 },
          "nodes": { "min_score": 0.05, "nms_iou": 0.8, "max_nodes": 128 },
          "score": 0.3, "over": 0.5
        }"#;
        let read: Description = serde_json::from_str(text).unwrap();
        assert_eq!((read.input.long_side, read.input.smallest_window), (1280, (320, 200)));
        assert_eq!((read.nodes.max_nodes, read.score), (128, 0.3));
    }

    /// Runs a version of the model on the windows of its folder `check` and compares what comes
    /// with what the reference in Python made of them. It needs ONNX Runtime and the model, so it
    /// runs only when asked: `BLOCKS_MODEL=<folder> ONNX_RUNTIME=<onnxruntime.dll> cargo test -p
    /// ainitshot -- --ignored --exact --nocapture finder::tests::the_model_finds_what_its_reference_finds`.
    #[test]
    #[ignore]
    fn the_model_finds_what_its_reference_finds() {
        let folder = PathBuf::from(std::env::var_os("BLOCKS_MODEL").expect("BLOCKS_MODEL names a version's folder"));
        let library = PathBuf::from(std::env::var_os("ONNX_RUNTIME").expect("ONNX_RUNTIME names the library"));
        let mut finder = Finder::load(&library, &folder).unwrap();
        let (mut all, mut alike) = (0, 0);
        for entry in fs::read_dir(folder.join("check")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "png") {
                continue;
            }
            let shot = Image::from_png(&fs::read(&path).unwrap()).unwrap();
            let expected: serde_json::Value = serde_json::from_slice(&fs::read(path.with_extension("json")).unwrap()).unwrap();
            let expected = expected["blocks"].as_array().unwrap();
            let started = Instant::now();
            let (found, _) = finder.found(&shot).unwrap();
            let seconds = started.elapsed().as_secs_f32();
            // The reference gives the frames that stay, with their nearest parents that stay.
            let least = finder.description.score;
            let kept_parent = |block: &Found| {
                let mut parent = block.parent;
                while parent != 0 && found[parent as usize - 1].score < least {
                    parent = found[parent as usize - 1].parent;
                }
                parent
            };
            let kept: Vec<&Found> = found.iter().filter(|block| block.score >= least).collect();
            let same = |block: &&serde_json::Value| {
                let want: Vec<i64> = block["box"].as_array().unwrap().iter().map(|side| side.as_i64().unwrap()).collect();
                kept.iter().any(|mine| {
                    let rect = [mine.rect.left, mine.rect.top, mine.rect.right, mine.rect.bottom];
                    rect.iter().zip(&want).all(|(a, b)| (i64::from(*a) - b).abs() <= 3)
                        && i64::from(kept_parent(mine)) == block["parent"].as_i64().unwrap()
                })
            };
            let matched = expected.iter().filter(same).count();
            println!(
                "{}: {}x{}, {} blocks of the reference, {} here, {matched} the same, in {seconds:.3} s",
                path.file_stem().unwrap().to_string_lossy(),
                shot.width,
                shot.height,
                expected.len(),
                kept.len(),
            );
            for block in expected.iter().filter(|block| !same(block)) {
                println!("  not found: {block}");
            }
            for mine in &kept {
                let rect = [mine.rect.left, mine.rect.top, mine.rect.right, mine.rect.bottom];
                let there = expected.iter().any(|block| {
                    let want: Vec<i64> = block["box"].as_array().unwrap().iter().map(|side| side.as_i64().unwrap()).collect();
                    rect.iter().zip(&want).all(|(a, b)| (i64::from(*a) - b).abs() <= 3)
                });
                if !there {
                    println!("  only here: {rect:?} parent {} score {:.3}", kept_parent(mine), mine.score);
                }
            }
            all += expected.len();
            alike += matched;
        }
        println!("{alike} of {all} alike");
        assert!(alike * 100 >= all * 95, "{alike} of {all}");
    }
}
