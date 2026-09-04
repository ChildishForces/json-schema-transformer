# Homebrew distribution

`jst.rb.template` is the source for the Homebrew formula. On every `v*` tag,
the Release workflow renders it with the release version and the sha256 of
each binary tarball, and attaches the result to the GitHub Release as
`jst.rb`.

To publish a release to Homebrew:

1. Create the tap repo once: `ChildishForces/homebrew-tap`.
2. After each release, copy the `jst.rb` asset from the GitHub Release into
   the tap as `Formula/jst.rb` and push.

Users then install with:

```sh
brew install childishforces/tap/jst
```
