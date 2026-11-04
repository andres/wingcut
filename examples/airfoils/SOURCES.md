# Airfoil sources

| File | Airfoil | Source |
|---|---|---|
| `naca2412.dat` | NACA 2412 | UIUC Airfoil Coordinates Database, https://m-selig.ae.illinois.edu/ads/coord/naca2412.dat |
| `naca2410.dat` | NACA 2410 | UIUC Airfoil Coordinates Database, https://m-selig.ae.illinois.edu/ads/coord/naca2410.dat |

Both `.dat` files are unmodified Selig-format files. `naca2410.dxf` is the same
NACA 2410 points scaled to a 200 mm chord, as one closed LWPOLYLINE starting at
the LE: a sample for DXF import. NACA 4-digit sections were published by
NACA (a US federal agency) and are in the public domain.

They are here only as working samples. For a real glider, put your own sections
(e.g. AG35/AG36 or SD7037 from the same database) next to `wing.toml` and point
the stations at them.
