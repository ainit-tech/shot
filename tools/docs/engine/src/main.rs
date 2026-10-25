//! `docs` — checks the documentation sources in `docs/` and builds data for the
//! documentation site (`tools/docs/ui`), for AI agents and for programs.

mod build;
mod check;
mod decision;
mod git;
mod glossary;
mod page;
mod site;
mod versions;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::build::Version;
use crate::check::Problem;
use crate::page::{Docs, LATEST, Urls};

const USAGE: &str = "\
Usage: docs <command>

Commands:
  check   Check the documentation sources and report problems
  build   Check the sources, then write site data to tools/docs/ui/public:
          the working tree as the latest version, and every commit on the main line
          that changed the docs as an older one
";

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool> {
    let command = std::env::args().nth(1);
    if !matches!(command.as_deref(), Some("check" | "build")) {
        eprint!("{USAGE}");
        return Ok(false);
    }

    let root = find_root()?;
    let docs = Docs::load(&root, Urls::for_version(LATEST))?;
    let problems = check::run(&docs);
    if !report(&docs, &problems) {
        return Ok(false);
    }
    if command.as_deref() == Some("build") {
        build_site(&root, &docs)?;
    }
    Ok(true)
}

fn build_site(root: &Path, latest_docs: &Docs) -> Result<()> {
    let public = root.join("tools").join("docs").join("ui").join("public");
    let mut out = build::Output::new(&public, build::fingerprint());

    // Every commit on the main line that changed the docs is a version, oldest first. Each is
    // compared with the version before it, and one built earlier by the same engine is kept.
    let planned = versions::number(&git::main_line(root, "docs"), &git::tags_by_commit(root));
    let mut built: Vec<(&versions::Planned, Version)> = Vec::new();
    // The docs of the version built last, if they were loaded, to compare the next one with.
    let mut loaded: Option<(String, Docs)> = None;
    for plan in &planned {
        let before = built.last().map(|(p, _)| *p);
        let version = Version {
            id: plan.id.clone(),
            commit: Some(plan.commit.clone()),
            date: Some(plan.date.clone()),
            rev: plan.hash.clone(),
            previous: before.map(|p| p.id.clone()),
            tagged: plan.tagged,
        };
        if out.reuse(&plan.id, &plan.commit, version.previous.as_deref()) {
            built.push((plan, version));
            loaded = None;
            continue;
        }
        let docs = match load_version(root, plan) {
            Ok(Some(docs)) => docs,
            // The first commits came before the docs had a site: they are not versions.
            Ok(None) => continue,
            Err(err) => {
                eprintln!("warning: version {} is skipped: {err:#}", plan.id);
                continue;
            }
        };
        let previous_docs = match (before, loaded.take()) {
            (Some(p), Some((id, docs))) if id == p.id => Some(docs),
            (Some(p), _) => load_version(root, p).ok().flatten(),
            (None, _) => None,
        };
        build::write_version(&docs, &version, root, &mut out, previous_docs.as_ref())?;
        built.push((plan, version));
        loaded = Some((plan.id.clone(), docs));
    }

    // The latest version is the working tree. With docs changes that are not committed yet it
    // is compared with the newest version; otherwise it is that version, compared with the one before.
    let dirty = !git::uncommitted_files(root, "docs").is_empty();
    let base = if dirty { built.last() } else { built.iter().rev().nth(1) }.map(|(p, _)| *p);
    let base_docs = match (base, loaded) {
        (Some(p), Some((id, docs))) if id == p.id => Some(docs),
        (Some(p), _) => load_version(root, p).ok().flatten(),
        (None, _) => None,
    };
    let (commit, date) = git::commit_of(root, "HEAD").unzip();
    let latest = Version {
        id: LATEST.to_string(),
        commit,
        date,
        rev: "HEAD".to_string(),
        previous: base.map(|p| p.id.clone()),
        tagged: false,
    };
    build::write_version(latest_docs, &latest, root, &mut out, base_docs.as_ref())?;

    let mut versions = vec![latest];
    versions.extend(built.into_iter().rev().map(|(_, version)| version));
    build::write_versions(&versions, &mut out)?;
    build::write_llms(latest_docs, &mut out)?;
    out.prune()?;

    let names: Vec<&str> = versions.iter().map(|v| v.id.as_str()).collect();
    println!("Site data written to {} (versions: {})", public.display(), names.join(", "));
    Ok(())
}

/// The docs as they were at a version's commit, or `None` if the docs had no site yet. Problems
/// in old versions are reported but do not stop the build: the past cannot be fixed.
fn load_version(root: &Path, plan: &versions::Planned) -> Result<Option<Docs>> {
    let source = root.join("target").join("docs-versions").join(&plan.id);
    git::export_docs(root, &plan.hash, &source)?;
    if !source.join("docs").join("site.toml").is_file() {
        return Ok(None);
    }
    let docs = Docs::load(&source, Urls::for_version(&plan.id))?;
    let errors = check::run(&docs).iter().filter(|p| p.error).count();
    if errors > 0 {
        eprintln!("warning: version {} has {errors} problem(s) and is built as is", plan.id);
    }
    Ok(Some(docs))
}

/// Finds the repository root: the nearest directory that contains `docs/site.toml`.
fn find_root() -> Result<PathBuf> {
    let current = std::env::current_dir()?;
    current
        .ancestors()
        .find(|dir| dir.join("docs/site.toml").is_file())
        .map(Path::to_path_buf)
        .context("docs/site.toml was not found in this directory or any parent")
}

/// Prints the problems and returns `false` if any of them is an error.
fn report(docs: &Docs, problems: &[Problem]) -> bool {
    for problem in problems {
        let level = if problem.error { "error" } else { "warning" };
        eprintln!("{level}: {}: {}", problem.place, problem.message);
    }
    let errors = problems.iter().filter(|p| p.error).count();
    let warnings = problems.len() - errors;
    let terms = docs.glossary.as_ref().map(|g| format!(" and {} terms", g.terms.len())).unwrap_or_default();
    eprintln!(
        "Checked {} pages in {} languages{terms}: {errors} errors, {warnings} warnings",
        docs.pages.len(),
        docs.site.languages.len()
    );
    errors == 0
}
