# Debian/Ubuntu distribution

`control.template` and `copyright` are the sources for the `.deb` packages.
On every `v*` tag, the Release workflow builds `jst_<version>_amd64.deb` and
`jst_<version>_arm64.deb` (via `dpkg-deb`) and attaches them to the GitHub
Release alongside the tarballs.

Users install with:

```sh
sudo apt install ./jst_<version>_<arch>.deb
```

These are standalone packages, not an apt repository — there are no automatic
updates. Publishing to a PPA or hosting an apt repo would be a separate step.
