//! Where atelier clones a repository from.

use atelier_core::Repo;

use crate::stand_in::{Reply, StandIn};

#[test]
fn a_clone_url_follows_gh_s_protocol_https_unless_it_is_set_to_ssh() {
    let gh = StandIn::new(&[
        Reply::Printed("git-protocol-https.txt"),
        Reply::Printed("git-protocol-ssh.txt"),
        Reply::Unreachable,
    ]);
    let github = gh.github();
    let repo = Repo("StevenJPx2/chauffeur".into());

    let urls = [(); 3].map(|()| github.clone_url(&repo).unwrap());

    assert_eq!(
        urls,
        [
            "https://github.com/StevenJPx2/chauffeur.git",
            "git@github.com:StevenJPx2/chauffeur.git",
            "https://github.com/StevenJPx2/chauffeur.git",
        ]
    );
    assert_eq!(
        gh.calls()[0],
        ["config", "get", "git_protocol", "-h", "github.com"]
    );
}
