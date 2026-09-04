# Homebrew distribution

`jst.rb.template` is the source for the Homebrew formula. On every `v*` tag,
the Release workflow renders it with the release version and the sha256 of
each binary tarball, and attaches the result to the GitHub Release as
`jst.rb`.

The workflow then pushes the rendered formula to the
`ChildishForces/homebrew-tap` repo (as `Formula/jst.rb`) via the
`HOMEBREW_TAP_TOKEN` secret — a fine-grained PAT with contents read/write on
the tap repo. If the secret is unset, the workflow warns and skips the push;
copy the `jst.rb` release asset into the tap manually in that case.

Users then install with:

```sh
brew install childishforces/tap/jst
```
