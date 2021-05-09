pkgname=key-mods
pkgver=1
pkgrel=1
pkgdesc="udevmon input remap utility"
arch=('x86_64' 'i686')
license=('GPL3')
depends=()
makedepends=(rustup)

build() {
	cd ..
  cargo build --release --locked --all-features --target-dir=target
}

check() {
	cd ..
  cargo test --release --locked --target-dir=target
}

package() {
	cd ..
  install -Dm 755 target/release/${pkgname} -t "${pkgdir}/usr/bin"

  install -Dm644 docs/man/key-mods.1 "$pkgdir/usr/share/man/man1/key-mods.1"
}
