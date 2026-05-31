# Static files (`public/`)

Semua file yang kamu taruh di folder ini akan disajikan di root situs oleh
`apiku serve`. Folder ini bisa diubah lewat `static_dir` pada blok `[web]` di
`config.toml`.

Gunakan untuk:

- File verifikasi mesin pencari / jaringan iklan
  - `google1234567890abcdef.html` -> `https://domain-kamu/google1234567890abcdef.html`
  - `BingSiteAuth.xml`
  - `ads.txt`, `app-ads.txt` (Google AdSense / AdX)
- `robots.txt`, `sitemap.xml`
- Logo / favicon custom yang dirujuk dari `[web].logo_url`

## Cara memasang logo custom

Ada dua cara:

### Cara cepat (otomatis terdeteksi)

Cukup taruh file bernama `logo.*` (atau `favicon.*`) langsung di folder ini —
server akan otomatis mendeteksinya saat start, tanpa perlu mengubah config:

```
public/logo.svg     (atau .png / .webp / .jpg / .jpeg / .gif / .ico)
```

Urutan prioritas: `logo.svg` > `logo.png` > `logo.webp` > `logo.jpg` >
`logo.jpeg` > `logo.gif` > `logo.ico`, lalu `favicon.*` dengan urutan sama.
Disarankan SVG atau PNG transparan, kira-kira persegi (mis. 64x64) agar pas di
header. Restart `apiku serve` setelah menaruh file.

### Cara manual (set di config)

Kalau ingin nama file lain atau URL absolut, set `logo_url` di `config.toml`.
Nilai ini selalu menang atas auto-deteksi:

```toml
[web]
site_name = "NamaSitusKamu"
logo_url  = "/brand.png"                 # file ada di public/brand.png
# atau: logo_url = "https://cdn.kamu/logo.svg"
```

- Path diawali `/` dan relatif terhadap root situs (bukan path disk), jadi
  `public/logo.svg` dirujuk sebagai `/logo.svg`.
- Kalau `logo_url` kosong DAN tidak ada file `logo.*`/`favicon.*`, dipakai
  logo gradien bawaan.

Logo otomatis dipakai di header, drawer mobile, dan favicon `<link rel="icon">`.

### Favicon terpisah (opsional)

Favicon mengikuti `logo_url`. Kalau ingin favicon yang berbeda dari logo
header, taruh `public/favicon.ico` lalu tambahkan tag-nya lewat `head_html`
di `config.toml`:

```toml
[web]
head_html = '<link rel="icon" href="/favicon.ico" sizes="any">'
```

## Catatan

- Hanya path satu segmen yang disajikan (`/logo.svg`, `/ads.txt`), bukan
  `/folder/file.svg`; percobaan path traversal ditolak. Taruh aset langsung
  di `public/`.
- Route API (`/api/...`), SPA (`/`), proxy gambar (`/img`), proxy HLS (`/hls`),
  dan `/tester` selalu diprioritaskan di atas file di sini.
- `.html` / `.txt` / `.xml` disajikan dengan cache pendek; gambar/font dapat
  cache panjang.
