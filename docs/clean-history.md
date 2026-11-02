# How to clean history

An alert says something on `main` failed a scan: a phone number, an address, a key. Removing the text from the file adds a new commit, and the old text stays in Git history, where anyone who can read the repository can still find it. This page is what to do about that. The alert's **How to clean history** button opens it.

## 1. Make the text harmless first

Cleaning history cannot reach copies that already exist: every Mac's clone, a fork, or GitHub's caches. So start with what you can change.

- **A secret** (a key, a token, a password): revoke or rotate it with whoever issued it. Once it no longer works, a copy left in history is harmless, and you may not need the rest of this page.
- **Personal information**, such as a child's phone number or address: you cannot rotate it, so cleaning history matters. Keep going.

## 2. Remove it from the file

**Open the file** shows it on GitHub at the commit the alert names. Remove the text on `main`: edit an issue in Wiffletree, which pushes the change like any other, or edit the file in GitHub's web editor. At its next pull each Mac scans the file again and closes its own alert once the rule no longer fires there.

## 3. Rewrite history

This replaces `main` with a version in which the text never existed. Everyone's next pull takes the new history. Do it from a terminal, on one Mac.

1. **Lift Protect history for the rewrite.** A repository with Protect history on refuses a force push, which is what this needs. Someone with admin rights on the repository edits the ruleset named Protect history on GitHub (**Settings → Rules → Rulesets**) and adds themselves to the bypass list, or turns the ruleset off for the next steps.
2. **Rewrite a fresh copy.** Install [git-filter-repo](https://github.com/newren/git-filter-repo), then, outside any folder Wiffletree uses:

   ```sh
   git clone --mirror git@github.com:OWNER/REPO.git
   cd REPO.git
   printf '%s\n' 'the exact text==>[removed]' > ../replacements.txt
   git filter-repo --replace-text ../replacements.txt
   git push --force --mirror
   rm ../replacements.txt
   ```

   Each line of `replacements.txt` is the text to remove, `==>`, and what to put in its place. Delete the file afterwards: it holds the text you are removing.
3. **Put Protect history back** as it was.
4. **Ask GitHub to drop its cached copies.** Old commits can still open by their ID on GitHub until it purges them. Contact GitHub Support with the repository and the commit IDs, as GitHub's guide to [removing sensitive data](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository) describes.

## 4. Every Mac

Each Mac's Wiffletree takes the rewritten `main` at its next pull, and its scan starts again where the old and new histories meet, so the rewrite raises no alert of its own. A clone made outside Wiffletree, such as one in a terminal, still holds the old commits until it is cloned again.

## Why Wiffletree does not do this for you

Rewriting `main` changes every commit after the slip, needs an admin to lift Protect history, and cannot reach copies already made. Wiffletree keeps history by design: every change is a commit anyone with access can see. So it raises the alert, never shows the text, and leaves the rewrite to a person who can decide it is worth it.
