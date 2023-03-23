# Maxwell

Displays Maxwell the cat on your 3DS.

## Building

You will the 3DS devKitPro tools and [cargo-3ds](https://github.com/rust3ds/cargo-3ds).

For release builds, run:

    cargo +nightly 3ds build -Zbuild-std=panic_abort,std -Zbuild-std-features=panic_immediate_abort --release

For debug builds, run:

    cargo +nightly 3ds build

## License

This project is licensed under the GNU General Public License, version 3 or later.

## Credits

Original model can be found [here](https://sketchfab.com/3d-models/dingus-the-cat-2ca7f3c1957847d6a145fc35de9046b0).

The music is Stockmarket by Weebls.

## Disclaimer

This project is not licensed by Nintendo.
