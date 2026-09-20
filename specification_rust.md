# Hexfront — specyfikacja implementacji w Rust (szkielet planu)

## Wstęp
Ta specyfikacja opisuje implementację gry w Rust. Implementacja jest dopiero planowana — ten dokument jest szkieletem, który uzupełniamy wraz z powstawaniem kodu. Plik [specification.md](specification.md) opisuje część implementacji niezależną od języka. Zasady gry pozostają jedynym źródłem prawdy w [rules.md](rules.md).

Edytor plansz jest na razie poza zakresem tej implementacji — istnieje tylko w wersji Pythonowej ([specification_of_map_editor.md](specification_of_map_editor.md)).

## Język i biblioteki
* stabilny Rust z systemu (rustc/cargo, obecnie 1.98.1), edycja 2024,
* crate binarny o nazwie `hexfront`,
* grafika, okno i obsługa wejścia: biblioteka **macroquad** (wersja z gałęzi 0.4, obecnie 0.4.16) pobierana z crates.io i pinowana w `Cargo.toml`,
* brak innych zależności na start; każdy dodatkowy crate wymaga uzasadnienia w komentarzu w `Cargo.toml` (np. generator liczb losowych do szumu AI zamiast własnej implementacji).

## Planowany układ kodu
```text
rust/Cargo.toml      manifest crate'a (Cargo.lock wersjonowany; target/ w .gitignore)
rust/src/main.rs     punkt wejścia gry (cargo run --release)
rust/src/            moduły odpowiadające pakietowi python/hexfront/
```

## Uruchamianie i testy
```bash
cd rust && cargo run --release   # gra
cd rust && cargo test            # testy
```

Testy logiki mają działać bez okna (bez inicjalizacji grafiki), tak jak testy headless w implementacji Pythonowej. Zakres: reguły z [rules.md](rules.md) (ruch, produkcja, walka, działka, wieże, eliminacja, AI) oraz format plansz — odczyt tych samych plików z `maps/` musi dawać identyczny stan początkowy jak w implementacji Pythonowej.

## Katalog maps
Ścieżkę katalogu `maps` wyznaczamy względem katalogu głównego repozytorium, a nie względem katalogu roboczego — to zasada z [specification.md](specification.md) (sekcja „Struktura repozytorium”). Punktem wyjścia jest katalog nadrzędny wobec `env!("CARGO_MANIFEST_DIR")`; przy uruchomieniu z gotowej binarki sprawdzamy kolejne katalogi nadrzędne wobec katalogu binarki. Nazwy modułu i funkcji ustalamy przy implementacji. Menu poziomów listuje wszystkie pliki `*.map` z tego katalogu (nazwa pliku jest nazwą poziomu), bez hardkodowania listy.

## Do doprecyzowania przy implementacji
* realizacja kontraktu wizualnego z [specification.md](specification.md): izometryczna kamera (`Camera2D`), rysowanie powierzchni trójkątami i programowy bufor głębokości (np. rasteryzacja do `Image`), kolejność przejść (podłoże, obiekty, cienie, zasięgi, drogi, liczniki jednostek);
* cache nieruchomego terenu i reguły jego unieważniania — ta sama lista zdarzeń co w implementacji Pythonowej ([specification_python.md](specification_python.md), sekcja „Renderowanie”);
* maszyna stanów aplikacji (menu poziomów, gra, pauza) i obsługa wejścia opisana w [specification.md](specification.md) (sekcja „Sterowanie”);
* determinizm symulacji i AI: ten sam seed i poziom dają ten sam przebieg, bez zależności od czasu zegara systemowego;
* docelowy FPS i sposób jego utrzymania przy panoramowaniu dużych widoków.

