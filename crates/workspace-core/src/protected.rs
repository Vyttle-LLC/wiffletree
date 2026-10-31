//! Which branches of a repository are protected: only the human merges into them. One rule,
//! shared by the host that enforces it, the context agents read and the Repositories page.
use crate::{Repository, Serialize, origin_branch};
use serde::Deserialize;

/// What GitHub said about a repository's branches in the daily read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubBranches {
    /// `None` for a repository with no commits yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// The merge methods GitHub allows, in the order a merge tries them.
    #[serde(default)]
    pub merge_methods: Vec<MergeMethod>,
    /// The active rulesets that target branches; `None` when GitHub would not list them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rulesets: Option<Vec<BranchRuleset>>,
    /// The patterns of classic branch protection rules; `None` when GitHub would not list them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection_rules: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeMethod {
    Squash,
    Merge,
    Rebase,
}
impl MergeMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Squash => "squash",
            Self::Merge => "merge commit",
            Self::Rebase => "rebase",
        }
    }
}

/// An active ruleset on branches, with its ref conditions as GitHub states them: `~ALL`,
/// `~DEFAULT_BRANCH` or a pattern under `refs/heads/`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchRuleset {
    pub name: String,
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}
const ALL_BRANCHES: &str = "~ALL";
const DEFAULT_BRANCH: &str = "~DEFAULT_BRANCH";
impl BranchRuleset {
    fn targets(&self, conditions: &[String], branch: &str, default: Option<&str>) -> bool {
        conditions.iter().any(|condition| match condition.as_str() {
            ALL_BRANCHES => true,
            DEFAULT_BRANCH => default == Some(branch),
            pattern => branch_matches(
                pattern.strip_prefix("refs/heads/").unwrap_or(pattern),
                branch,
            ),
        })
    }
    fn applies(&self, branch: &str, default: Option<&str>) -> bool {
        self.targets(&self.include, branch, default)
            && !self.targets(&self.exclude, branch, default)
    }
    /// It targets every branch with no exception.
    fn covers_all(&self) -> bool {
        self.exclude.is_empty() && self.include.iter().any(|c| c == ALL_BRANCHES)
    }
}

/// Why a row of the protected branches is there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Protection {
    /// The default branch on GitHub, the base's branch on `origin`, or both: never removable.
    Always { default_branch: bool, base: bool },
    /// A ruleset on GitHub; changed only there.
    GitHub {
        ruleset: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        except: Vec<String>,
    },
    /// A classic branch protection rule on GitHub; changed only there.
    GitHubRule,
    /// An entry of the human's own.
    Entry,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProtectedRow {
    /// A branch, a pattern, or "all branches" for a ruleset on every branch.
    pub branches: String,
    #[serde(flatten)]
    pub protection: Protection,
}
impl ProtectedRow {
    /// Why the row is protected, as the Repositories page says it.
    pub fn reason(&self) -> String {
        match &self.protection {
            Protection::Always {
                default_branch: true,
                base: true,
            } => "Default branch on GitHub, and this repository’s base · always protected".into(),
            Protection::Always { base: true, .. } => {
                "This repository’s base · always protected while it is the base".into()
            }
            Protection::Always { .. } => "Default branch on GitHub · always protected".into(),
            Protection::GitHub { ruleset, except } => {
                let except = match except.is_empty() {
                    true => String::new(),
                    false => format!(" except {}", except.join(", ")),
                };
                format!("GitHub ruleset “{ruleset}”{except} · read from GitHub, change it there")
            }
            Protection::GitHubRule => {
                "GitHub branch protection · read from GitHub, change it there".into()
            }
            Protection::Entry => "Added by you".into(),
        }
    }
}

/// A repository's protected branches: the derived ones, GitHub's and the human's entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedBranches<'a> {
    default_branch: Option<&'a str>,
    base: Option<&'a str>,
    /// `None` until GitHub has listed the repository's rulesets.
    rulesets: Option<&'a [BranchRuleset]>,
    protection_rules: Option<&'a [String]>,
    entries: &'a [String],
}
impl<'a> ProtectedBranches<'a> {
    pub fn of(repository: &'a Repository) -> Self {
        let branches = repository
            .github
            .as_ref()
            .and_then(|github| github.facts.as_ref())
            .and_then(|facts| facts.branches.as_ref());
        Self {
            default_branch: branches.and_then(|b| b.default_branch.as_deref()),
            base: origin_branch(&repository.base),
            rulesets: branches.and_then(|b| b.rulesets.as_deref()),
            protection_rules: branches.and_then(|b| b.protection_rules.as_deref()),
            entries: &repository.protected_branches,
        }
    }
    /// Whether GitHub has told which branches it protects; until then the page says so.
    pub fn github_read(&self) -> bool {
        self.rulesets.is_some() && self.protection_rules.is_some()
    }
    fn protection_rules(&self) -> &'a [String] {
        self.protection_rules.unwrap_or_default()
    }
    fn always(&self, branch: &str) -> bool {
        self.default_branch == Some(branch) || self.base == Some(branch)
    }
    fn rulesets(&self) -> &'a [BranchRuleset] {
        self.rulesets.unwrap_or_default()
    }
    pub fn protects(&self, branch: &str) -> bool {
        self.always(branch)
            || self
                .entries
                .iter()
                .any(|entry| branch_matches(entry, branch))
            || (self.rulesets().iter()).any(|r| r.applies(branch, self.default_branch))
            || (self.protection_rules().iter()).any(|rule| branch_matches(rule, branch))
    }
    /// A ruleset, a branch protection rule or an entry protects every branch, so the host can
    /// merge nothing in the repository.
    pub fn every_branch(&self) -> bool {
        // Only stars, `**` among them, match every name.
        let all = |pattern: &String| pattern.contains("**") && pattern.chars().all(|c| c == '*');
        self.rulesets().iter().any(BranchRuleset::covers_all)
            || self.protection_rules().iter().any(all)
            || self.entries.iter().any(all)
    }
    /// The rows in the order the page lists them: always protected, GitHub's, then the human's.
    pub fn rows(&self) -> Vec<ProtectedRow> {
        // A base that is also the default branch is one row.
        let base = self.base.filter(|base| self.default_branch != Some(base));
        let mut rows: Vec<ProtectedRow> = (self.default_branch.into_iter().chain(base))
            .map(|branch| ProtectedRow {
                branches: branch.into(),
                protection: Protection::Always {
                    default_branch: self.default_branch == Some(branch),
                    base: self.base == Some(branch),
                },
            })
            .collect();
        for ruleset in self.rulesets() {
            let shown = |condition: &String| match condition.as_str() {
                ALL_BRANCHES => "all branches".to_owned(),
                DEFAULT_BRANCH => self.default_branch.unwrap_or("the default branch").into(),
                pattern => pattern
                    .strip_prefix("refs/heads/")
                    .unwrap_or(pattern)
                    .into(),
            };
            let except: Vec<String> = ruleset.exclude.iter().map(shown).collect();
            rows.extend(ruleset.include.iter().map(|condition| ProtectedRow {
                branches: shown(condition),
                protection: Protection::GitHub {
                    ruleset: ruleset.name.clone(),
                    except: except.clone(),
                },
            }));
        }
        rows.extend(self.protection_rules().iter().map(|rule| ProtectedRow {
            branches: rule.clone(),
            protection: Protection::GitHubRule,
        }));
        rows.extend(self.entries.iter().map(|entry| ProtectedRow {
            branches: entry.clone(),
            protection: Protection::Entry,
        }));
        rows
    }
    /// Why the human cannot add `entry`, given the repository's remotes; `None` when it can be
    /// added.
    pub fn refusal(&self, entry: &str, remotes: &[String]) -> Option<String> {
        if entry.is_empty() {
            return Some("Enter a branch or pattern".into());
        }
        if entry.contains(char::is_whitespace) {
            return Some("A branch or pattern has no spaces".into());
        }
        if entry.starts_with("refs/") {
            return Some(format!(
                "Name the branch without “{}”, as it is on origin",
                entry.split_inclusive('/').take(2).collect::<String>()
            ));
        }
        let (first, rest) = entry.split_once('/').unwrap_or((entry, ""));
        if remotes.iter().any(|remote| remote == first) {
            let already = !rest.is_empty() && (self.always(rest) || self.listed(rest));
            return Some(match (rest, already) {
                ("", _) => "Name a branch, not a remote".into(),
                (_, true) => {
                    format!("Name a branch, not a remote branch: “{rest}” is already protected")
                }
                (_, false) => format!("Name a branch, not a remote branch, such as “{rest}”"),
            });
        }
        if let Err(problem) = parse(entry) {
            return Some(format!(
                "Not a valid pattern: {problem}. Use * for any part of one name, ** across /, \
                 or [0-9] for one character"
            ));
        }
        if !is_pattern(entry) && !is_branch_name(entry) {
            return Some(format!("“{entry}” is not a valid branch name"));
        }
        if self.listed(entry) {
            return Some(format!("“{entry}” is already on the list"));
        }
        let why = match (self.default_branch == Some(entry), self.base == Some(entry)) {
            (true, true) => "it is the default branch on GitHub and this repository’s base",
            (true, false) => "it is the default branch on GitHub",
            (false, true) => "it is this repository’s base",
            (false, false) => return None,
        };
        Some(format!("“{entry}” is already protected: {why}"))
    }
    fn listed(&self, entry: &str) -> bool {
        self.entries.iter().any(|e| e == entry)
    }
}

/// Whether a host merge into the repository is possible at all, as agents and the page see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Merging {
    /// Into any branch that is not protected.
    IntoUnprotected,
    /// The repository does not publish without asking, so the host merges nothing there.
    NotAllowed,
    /// A ruleset on GitHub protects every branch.
    EveryBranchProtected,
}
impl Merging {
    pub fn of(repository: &Repository, allowed: &[String]) -> Self {
        if !crate::is_allowed(repository, allowed) {
            Self::NotAllowed
        } else if ProtectedBranches::of(repository).every_branch() {
            Self::EveryBranchProtected
        } else {
            Self::IntoUnprotected
        }
    }
    pub fn possible(&self) -> bool {
        *self == Self::IntoUnprotected
    }
}

/// The first merge method GitHub allows, of squash, merge commit and rebase, once it has said.
pub fn next_merge_method(repository: &Repository) -> Option<MergeMethod> {
    let facts = repository.github.as_ref()?.facts.as_ref()?;
    facts.branches.as_ref()?.merge_methods.iter().min().copied()
}

fn is_pattern(entry: &str) -> bool {
    entry.contains(['*', '?', '['])
}

/// Whether `name` is a valid branch name, by the rules of `git check-ref-format --branch`.
pub fn is_branch_name(name: &str) -> bool {
    !name.is_empty()
        && name != "@"
        && name != "HEAD"
        && !name.starts_with(['-', '/'])
        && !name.ends_with(['/', '.'])
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && !name
            .chars()
            .any(|c| c.is_ascii_control() || " ~^:?*[\\".contains(c))
        && name
            .split('/')
            .all(|part| !part.starts_with('.') && !part.ends_with(".lock"))
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Char(char),
    /// `?`: one character except `/`.
    One,
    /// `*`: any run except `/`.
    Star,
    /// `**`: any run, `/` included.
    DeepStar,
    /// `[...]`: one character except `/`, in or, negated, out of the ranges.
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

fn parse(pattern: &str) -> Result<Vec<Token>, &'static str> {
    let mut tokens = vec![];
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        tokens.push(match c {
            '*' if chars.next_if_eq(&'*').is_some() => Token::DeepStar,
            '*' => Token::Star,
            '?' => Token::One,
            '[' => {
                let negated = chars.next_if(|c| *c == '!' || *c == '^').is_some();
                let mut ranges = vec![];
                // A `]` first in the class is one of its characters.
                let mut first = true;
                loop {
                    let Some(c) = chars.next() else {
                        return Err("“[” is never closed");
                    };
                    if c == ']' && !first {
                        break;
                    }
                    first = false;
                    let end = match chars.peek() {
                        Some('-') => {
                            chars.next();
                            match chars.next_if(|c| *c != ']') {
                                Some(end) => end,
                                None => {
                                    ranges.push(('-', '-'));
                                    c
                                }
                            }
                        }
                        _ => c,
                    };
                    ranges.push((c, end));
                }
                Token::Class { negated, ranges }
            }
            c => Token::Char(c),
        });
    }
    Ok(tokens)
}

/// Whether `branch` matches the exact name or glob `pattern`, as GitHub matches branch
/// protection patterns. A pattern that cannot be read matches nothing.
pub fn branch_matches(pattern: &str, branch: &str) -> bool {
    let branch: Vec<char> = branch.chars().collect();
    parse(pattern).is_ok_and(|tokens| Matcher::new(&tokens, &branch).at(0, 0))
}

/// Matches tokens against a name, remembering each (token, character) position it has decided,
/// so stars in a row take polynomial time rather than exponential.
struct Matcher<'a> {
    tokens: &'a [Token],
    name: &'a [char],
    decided: Vec<Option<bool>>,
}
impl<'a> Matcher<'a> {
    fn new(tokens: &'a [Token], name: &'a [char]) -> Self {
        let decided = vec![None; (tokens.len() + 1) * (name.len() + 1)];
        Self {
            tokens,
            name,
            decided,
        }
    }
    fn at(&mut self, token: usize, at: usize) -> bool {
        let slot = token * (self.name.len() + 1) + at;
        if let Some(decided) = self.decided[slot] {
            return decided;
        }
        let matched = self.decide(token, at);
        self.decided[slot] = Some(matched);
        matched
    }
    fn decide(&mut self, token: usize, at: usize) -> bool {
        let (tokens, name) = (self.tokens, self.name);
        let Some(current) = tokens.get(token) else {
            return at == name.len();
        };
        let next = name.get(at).copied();
        let one = |test: &dyn Fn(char) -> bool| next.is_some_and(|c| c != '/' && test(c));
        match current {
            Token::Char(c) => next == Some(*c) && self.at(token + 1, at + 1),
            Token::One => one(&|_| true) && self.at(token + 1, at + 1),
            Token::Class { negated, ranges } => {
                one(&|c| ranges.iter().any(|(a, b)| (*a..=*b).contains(&c)) != *negated)
                    && self.at(token + 1, at + 1)
            }
            Token::Star => (at..=name.len())
                .take_while(|&i| i == at || name[i - 1] != '/')
                .any(|i| self.at(token + 1, i)),
            // `**/` also stands for no folder at all, so `qa/**/*` matches `qa/foo`.
            Token::DeepStar => {
                (tokens.get(token + 1) == Some(&Token::Char('/')) && self.at(token + 2, at))
                    || (at..=name.len()).any(|i| self.at(token + 1, i))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GitHubFacts, GitHubRepository};

    #[test]
    fn patterns_match_as_on_github() {
        let cases: &[(&str, &[&str], &[&str])] = &[
            ("main", &["main"], &["main2", "Main", "x/main"]),
            (
                "release/*",
                &["release/2026.10"],
                &["release/2026/10", "releases/x", "release"],
            ),
            (
                "release/**",
                &["release/2026/10", "release/x"],
                &["releases/x"],
            ),
            // GitHub's own examples for branch protection and rulesets.
            (
                "*release*",
                &["release", "prerelease", "releases"],
                &["feature/release"],
            ),
            ("qa/**/*", &["qa/foo", "qa/foo/bar"], &["qa"]),
            (
                "releases/**/*",
                &["releases/v1/x", "releases/v1"],
                &["release/v1"],
            ),
            ("v[0-9]", &["v1"], &["va", "v10"]),
            ("v[!0-9]", &["va"], &["v1", "v/"]),
            ("v[^0-9]", &["va"], &["v1"]),
            ("v?", &["v1"], &["v/", "v12"]),
            ("[]a]x", &["]x", "ax"], &["bx"]),
            ("x[a-]", &["xa", "x-"], &["xb"]),
            ("**", &["any/thing", "main"], &[]),
        ];
        for (pattern, matched, unmatched) in cases {
            for branch in *matched {
                assert!(
                    branch_matches(pattern, branch),
                    "{pattern} should match {branch}"
                );
            }
            for branch in *unmatched {
                assert!(
                    !branch_matches(pattern, branch),
                    "{pattern} should not match {branch}"
                );
            }
        }
        assert!(!branch_matches("release/[0-9", "release/[0-9"));
        // Stars in a row that cannot match decide at once rather than backtracking.
        let stars = format!("{}b", "*a".repeat(24));
        let started = std::time::Instant::now();
        assert!(!branch_matches(&stars, &"a".repeat(48)));
        assert!(branch_matches(&"**a".repeat(40), &"a/".repeat(60)[..119]));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    fn repository(base: &str, branches: Option<GitHubBranches>, entries: &[&str]) -> Repository {
        Repository {
            id: "r".into(),
            name: "cortex".into(),
            path: "/r".into(),
            base: base.into(),
            github: Some(GitHubRepository {
                owner: "Sage-Social".into(),
                name: "sagechat-cortex".into(),
                facts: Some(GitHubFacts {
                    name_with_owner: "Sage-Social/sagechat-cortex".into(),
                    organization: true,
                    fork: false,
                    parent: None,
                    public: false,
                    checked_at: 0,
                    branches,
                }),
                problem: None,
            }),
            origin_problem: None,
            checks: None,
            protected_branches: entries.iter().map(|e| e.to_string()).collect(),
        }
    }
    fn on_github(default: &str, rulesets: Vec<BranchRuleset>) -> Option<GitHubBranches> {
        Some(GitHubBranches {
            default_branch: Some(default.into()),
            merge_methods: vec![MergeMethod::Rebase, MergeMethod::Squash],
            rulesets: Some(rulesets),
            protection_rules: Some(vec![]),
        })
    }
    fn ruleset(name: &str, include: &[&str], exclude: &[&str]) -> BranchRuleset {
        BranchRuleset {
            name: name.into(),
            include: include.iter().map(|c| c.to_string()).collect(),
            exclude: exclude.iter().map(|c| c.to_string()).collect(),
        }
    }

    #[test]
    fn the_default_branch_and_the_base_are_protected_and_nothing_else() {
        let cortex = repository("origin/develop", on_github("main", vec![]), &[]);
        let protected = ProtectedBranches::of(&cortex);
        assert!(protected.protects("main") && protected.protects("develop"));
        assert!(!protected.protects("feature/x"));
        // The old base stays protected only when something else protects it.
        let moved = repository("origin/main", on_github("main", vec![]), &[]);
        assert!(!ProtectedBranches::of(&moved).protects("develop"));
        let listed = repository("origin/main", on_github("main", vec![]), &["develop"]);
        assert!(ProtectedBranches::of(&listed).protects("develop"));
        // A local base protects nothing.
        let local = repository("develop", on_github("main", vec![]), &[]);
        assert!(!ProtectedBranches::of(&local).protects("develop"));
    }

    #[test]
    fn rulesets_protect_what_they_target_and_one_on_every_branch_stops_merging() {
        let releases = ruleset("Releases", &["refs/heads/release/**"], &[]);
        let main = ruleset("main", &["~DEFAULT_BRANCH"], &[]);
        let cortex = repository("origin/main", on_github("trunk", vec![releases, main]), &[]);
        let protected = ProtectedBranches::of(&cortex);
        assert!(protected.protects("release/2026.11") && protected.protects("trunk"));
        assert!(!protected.protects("feature/x") && !protected.every_branch());
        let rows = protected.rows();
        assert_eq!(rows[2].branches, "release/**");
        assert_eq!(rows[3].branches, "trunk");

        let except = ruleset("Most", &["~ALL"], &["refs/heads/scratch/*"]);
        let some = repository("origin/main", on_github("main", vec![except]), &[]);
        let protected = ProtectedBranches::of(&some);
        assert!(protected.protects("feature/x") && !protected.protects("scratch/y"));
        assert!(!protected.every_branch());

        let all = repository(
            "origin/main",
            on_github("main", vec![ruleset("All", &["~ALL"], &[])]),
            &[],
        );
        let protected = ProtectedBranches::of(&all);
        assert!(protected.every_branch());
        assert_eq!(protected.rows()[1].branches, "all branches");
        let allowed = ["Sage-Social".to_owned()];
        assert_eq!(Merging::of(&all, &allowed), Merging::EveryBranchProtected);
        assert_eq!(Merging::of(&cortex, &allowed), Merging::IntoUnprotected);
        // An entry or a classic rule of `**` protects every branch too; `release/**` does not.
        let starred = repository("origin/main", on_github("main", vec![]), &["**"]);
        assert_eq!(
            Merging::of(&starred, &allowed),
            Merging::EveryBranchProtected
        );
        let mut rule = on_github("main", vec![]);
        rule.as_mut().unwrap().protection_rules = Some(vec!["***".into()]);
        let ruled = repository("origin/main", rule, &["release/**"]);
        assert_eq!(Merging::of(&ruled, &allowed), Merging::EveryBranchProtected);
        let some = repository(
            "origin/main",
            on_github("main", vec![]),
            &["release/**", "*"],
        );
        assert_eq!(Merging::of(&some, &allowed), Merging::IntoUnprotected);
        assert_eq!(Merging::of(&cortex, &[]), Merging::NotAllowed);
    }

    #[test]
    fn the_default_branch_and_base_show_as_one_row_when_they_are_the_same() {
        let wiffletree = repository("origin/main", on_github("main", vec![]), &["release/*"]);
        let rows = ProtectedBranches::of(&wiffletree).rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].reason(),
            "Default branch on GitHub, and this repository’s base · always protected"
        );
        assert_eq!(rows[1].protection, Protection::Entry);
        assert_eq!(next_merge_method(&wiffletree), Some(MergeMethod::Squash));
    }

    #[test]
    fn entries_are_refused_with_the_reason() {
        let cortex = repository("origin/develop", on_github("main", vec![]), &["release/*"]);
        let protected = ProtectedBranches::of(&cortex);
        let remotes = ["origin".to_owned(), "upstream".to_owned()];
        let refusal = |entry| protected.refusal(entry, &remotes).unwrap_or_default();
        assert!(refusal("release/[0-9").contains("“[” is never closed"));
        assert_eq!(
            refusal("origin/main"),
            "Name a branch, not a remote branch: “main” is already protected"
        );
        assert!(refusal("upstream/hotfix").contains("such as “hotfix”"));
        assert!(refusal("origin").contains("not a remote"));
        assert!(refusal("").contains("Enter"));
        assert!(refusal("hot fix").contains("no spaces"));
        assert!(refusal("refs/heads/main").contains("without “refs/heads/”"));
        assert!(refusal("release/*").contains("already on the list"));
        assert!(refusal("develop").contains("base"));
        assert!(refusal("main").contains("default branch"));
        for invalid in ["a..b", "-x", "x.lock", "x/", "a//b", "a@{b", "HEAD", "x."] {
            assert!(
                refusal(invalid).contains("not a valid branch name"),
                "{invalid}"
            );
        }
        for accepted in ["hotfix/*", "release/**", "demo", "v[0-9]*", "feature/a.b"] {
            assert_eq!(protected.refusal(accepted, &remotes), None, "{accepted}");
        }
    }

    #[test]
    fn a_classic_branch_protection_rule_protects_what_it_matches() {
        let mut branches = on_github("main", vec![]);
        branches.as_mut().unwrap().protection_rules = Some(vec!["release/*".into()]);
        let cortex = repository("origin/main", branches, &[]);
        let protected = ProtectedBranches::of(&cortex);
        assert!(protected.protects("release/2026.11") && !protected.protects("feature/x"));
        assert_eq!(protected.rows()[1].protection, Protection::GitHubRule);
        assert!(protected.github_read());
    }

    #[test]
    fn a_repository_github_has_not_described_protects_its_base_and_entries() {
        let unread = repository("origin/main", None, &["release/*"]);
        let protected = ProtectedBranches::of(&unread);
        assert!(!protected.github_read());
        assert!(protected.protects("main") && protected.protects("release/1"));
        assert_eq!(next_merge_method(&unread), None);
    }
}
