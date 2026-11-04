# Airfoil sources

| File | Airfoil | Source |
|---|---|---|
| `naca2412.dat` | NACA 2412 | UIUC Airfoil Coordinates Database, https://m-selig.ae.illinois.edu/ads/coord/naca2412.dat |
| `naca2410.dat` | NACA 2410 | UIUC Airfoil Coordinates Database, https://m-selig.ae.illinois.edu/ads/coord/naca2410.dat |

Both are unmodified Selig-format files. NACA 4-digit sections were published by
NACA (a US federal agency) and are in the public domain.

They are here only as working samples. For a real glider, put your own sections
(e.g. AG35/AG36 or SD7037 from the same database) next to `wing.toml` and point
the stations at them.
