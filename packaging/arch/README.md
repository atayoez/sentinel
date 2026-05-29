# Arch packaging

Two PKGBUILDs:

- `PKGBUILD` — stable release. Pulls the GitHub release tarball
  (`v$pkgver`). Submit to AUR as **`sentinel-cosmic`**.
- `(removed)` — VCS package. Pulls main branch HEAD. Submit as
  **`sentinel-git`**.

## Submitting to AUR (first time)

```bash
# 1. Create the SSH key + AUR account at https://aur.archlinux.org/

# 2. Clone an empty AUR repo (the name is the package name).
git clone ssh://aur@aur.archlinux.org/sentinel-cosmic.git aur-sentinel-cosmic
cd aur-sentinel-cosmic

# 3. Copy in the PKGBUILD and generate .SRCINFO.
cp ../packaging/arch/PKGBUILD .
# Refresh the source checksum to a real value (replaces SKIP):
updpkgsums                          # from `pacman-contrib`
makepkg --printsrcinfo > .SRCINFO

# 4. Verify it builds in a clean chroot.
makepkg -si --clean

# 5. Commit and push.
git add PKGBUILD .SRCINFO
git commit -m "sentinel-cosmic ${pkgver}-${pkgrel}: initial release"
git push origin master

# 6. Repeat for sentinel-git in a separate clone.
git clone ssh://aur@aur.archlinux.org/sentinel-git.git aur-sentinel-cosmic-git
cd aur-sentinel-cosmic-git
cp ../packaging/arch/(removed) PKGBUILD
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO
git commit -m "sentinel-git: initial release"
git push origin master
```

## Updating for a new release

```bash
cd aur-sentinel-cosmic
# Bump pkgver / pkgrel in PKGBUILD, refresh checksum:
updpkgsums
makepkg --printsrcinfo > .SRCINFO
git commit -am "sentinel-cosmic $(grep -m1 ^pkgver= PKGBUILD | cut -d= -f2)-1"
git push
```

## Local test (no AUR push needed)

```bash
cd packaging/arch
makepkg -si              # build + install in one step
```
