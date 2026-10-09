// SPDX-License-Identifier: MIT

mod support;
use flight_workspaces::*;
use std::os::unix::fs::PermissionsExt;
use support::*;

fn probe() -> FsProbe {
    FsProbe::new(None)
}

fn path(dir: &std::path::Path) -> String {
    dir.to_string_lossy().into_owned()
}

#[test]
fn a_directory_that_never_existed_is_confirmed_missing_and_is_not_created() {
    let base = scratch("roots-missing");
    let root = base.join("proj");
    assert_eq!(probe().probe("h", &path(&root)), RootState::Missing);
    assert!(!root.exists());
}

#[test]
fn a_missing_ancestor_is_unverified_not_missing() {
    let base = scratch("roots-ancestor");
    let root = base.join("gone").join("proj");
    assert!(matches!(
        probe().probe("h", &path(&root)),
        RootState::Unverified { .. }
    ));
    assert!(!base.join("gone").exists());
}

#[test]
fn a_file_is_not_a_directory() {
    let base = scratch("roots-file");
    std::fs::write(base.join("f"), "x").unwrap();
    assert_eq!(
        probe().probe("h", &path(&base.join("f"))),
        RootState::NotADirectory
    );
}

#[test]
fn a_directory_behind_missing_permission_is_a_permission_failure() {
    let base = scratch("roots-perm");
    let locked = base.join("locked");
    std::fs::create_dir_all(locked.join("proj")).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let got = probe().probe("h", &path(&locked.join("proj")));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Running as root can read anything; there is nothing to assert then.
    if std::fs::read_dir(&locked).is_ok() {
        return;
    }
    assert_eq!(got, RootState::PermissionDenied);
}

#[test]
fn a_relative_path_cannot_be_verified() {
    assert!(matches!(
        probe().probe("h", "proj"),
        RootState::Unverified { .. }
    ));
    assert!(matches!(
        probe().probe("h", "~/proj"),
        RootState::Unverified { .. }
    ));
}

#[test]
fn tilde_resolves_against_the_hosts_home() {
    let home = scratch("roots-home");
    std::fs::create_dir(home.join("proj")).unwrap();
    assert!(matches!(
        FsProbe::new(Some(home)).probe("h", "~/proj"),
        RootState::Present { .. }
    ));
}

#[test]
fn a_reappearing_path_is_the_same_workspace_only_if_the_directory_is_the_same() {
    let base = scratch("roots-reappear");
    let root = base.join("proj");
    std::fs::create_dir(&root).unwrap();
    let mut spec = RootSpec::new(path(&root));
    assert_eq!(
        spec.check(&probe().probe("h", &spec.path)),
        RootCheck::FirstSighting
    );
    spec.record(&probe().probe("h", &spec.path));
    assert_eq!(
        spec.check(&probe().probe("h", &spec.path)),
        RootCheck::Verified
    );

    // It disappears: unavailable, and the record survives.
    let moved = base.join("elsewhere");
    std::fs::rename(&root, &moved).unwrap();
    let gone = spec.check(&probe().probe("h", &spec.path));
    assert_eq!(gone, RootCheck::Unavailable(RootState::Missing));
    assert!(spec.identity.is_some());

    // The same directory comes back: verified.
    std::fs::rename(&moved, &root).unwrap();
    assert_eq!(
        spec.check(&probe().probe("h", &spec.path)),
        RootCheck::Verified
    );

    // A different directory appears at the path: a mismatch for the user to decide, not a match.
    std::fs::rename(&root, &moved).unwrap();
    std::fs::create_dir(&root).unwrap();
    let got = spec.check(&probe().probe("h", &spec.path));
    assert!(matches!(got, RootCheck::Changed(_)), "{got:?}");
    assert!(!got.usable());
}

#[test]
fn accepting_a_changed_root_is_an_explicit_rerecord() {
    let mut spec = RootSpec::new("/p");
    spec.record(&present(1));
    assert!(matches!(spec.check(&present(2)), RootCheck::Changed(_)));
    spec.record(&present(2));
    assert_eq!(spec.check(&present(2)), RootCheck::Verified);
}

#[test]
fn an_independent_clone_and_a_plain_directory_are_both_ordinary_roots() {
    let base = scratch("roots-git");
    let plain = base.join("plain");
    let clone = base.join("clone");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::create_dir_all(clone.join(".git")).unwrap();
    let git = |p: &std::path::Path| match probe().probe("h", &path(p)) {
        RootState::Present { git, .. } => git,
        other => panic!("{other:?}"),
    };
    assert_eq!(git(&plain), GitMarker::Absent);
    assert_eq!(git(&clone), GitMarker::Directory);
}

#[test]
fn a_dot_git_file_is_not_assumed_to_be_a_directory() {
    let base = scratch("roots-gitfile");
    let wt = base.join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: /repo/.git/worktrees/wt\n").unwrap();
    let RootState::Present { git, .. } = probe().probe("h", &path(&wt)) else {
        panic!()
    };
    assert_eq!(
        git,
        GitMarker::File {
            gitdir: Some("/repo/.git/worktrees/wt".to_owned())
        }
    );
    // Invalid pointer metadata is reported as such; the target need not exist and is not followed.
    std::fs::write(wt.join(".git"), "garbage").unwrap();
    let RootState::Present { git, .. } = probe().probe("h", &path(&wt)) else {
        panic!()
    };
    assert_eq!(git, GitMarker::File { gitdir: None });
}

#[test]
fn a_clone_that_becomes_a_pointer_file_is_flagged_as_a_layout_change() {
    let base = scratch("roots-layout");
    let dir = base.join("p");
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    let mut spec = RootSpec::new(path(&dir));
    spec.record(&probe().probe("h", &spec.path));
    std::fs::remove_dir(dir.join(".git")).unwrap();
    std::fs::write(dir.join(".git"), "gitdir: /x\n").unwrap();
    assert!(matches!(
        spec.check(&probe().probe("h", &spec.path)),
        RootCheck::Changed(_)
    ));
}
