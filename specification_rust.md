# Hexfront — specyfikacja implementacji w Rust

## Wstęp
Ta specyfikacja opisuje implementację gry w Rust. Implementacja gry i edytora plansz istnieje i jest kompletna (gra + edytor + testy w `rust/`); edytor plansz jest integralną częścią tej samej aplikacji (stan edytora, nie osobny program) i opisuje go sekcja „Edytor plansz” w tym pliku. Plik [specification.md](specification.md) opisuje część implementacji niezależną od języka. Zasady gry pozostają jedynym źródłem prawdy w [rules.md](rules.md). Wiążą nas wspólny kontrakt z [specification.md](specification.md) i format planszy z [specification_of_map_format.md](specification_of_map_format.md); podział modułów, nazwy i rozwiązania techniczne są decyzją tej implementacji.

Ten dokument opisuje **decyzje** implementacji w Rust, a nie szczegóły wykonania. Zapisuje wybór technologii, mapę modułów i uzasadnienie decyzji; opisy algorytmów, wartości w px, nazwy buforów i wewnętrznych funkcji należą do dokumentacji kodu w `rust/src/` (modułowe `//!` i doc-komentarze), która jest dla tej implementacji dokumentacją wykonawczą.

Pliki `maps/*.map` są wersjonowane, a nowe plansze tworzy się wyłącznie edytorem wbudowanym w grę (sekcja „Edytor plansz” poniżej) — generator proceduralny poziomów nie wchodzi w zakres tej implementacji, która mapy jedynie czyta i zapisuje.

Decyzje zapisane w tej specyfikacji (nazwy modułów, nazwy stałych, rozwiązania techniczne) można zmieniać przy implementacji, o ile jest ku temu konkretny powód — zawsze wraz z aktualizacją tej specyfikacji w tym samym commicie (zasada wspólna z [specification.md](specification.md), sekcja „Kod”).

## Język i biblioteki
* stabilny Rust z systemu (rustc/cargo), edycja 2024,
* crate binarny o nazwie `hexfront`,
* grafika, okno i obsługa wejścia: biblioteka **macroquad** (gałąź 0.4) pobierana z crates.io i pinowana w `Cargo.toml`,
* dźwięk: opcjonalna funkcja `audio` tej samej biblioteki (jej własny backend `quad-snd`) oraz dwie skrzynie uzasadnione w `Cargo.toml` — `lewton` do dekodowania Ogg Vorbis i `resampler` do korekty próbkowania, bo własna ścieżka backendu jest wadliwa. Nagrania leżą w katalogu `sounds/` w oryginalnym kodowaniu Ogg Vorbis (pochodzenie i licencja w `sounds/README.md`),
* brak innych zależności — generator liczb losowych dla AI jest własny (`rng.rs`); każdy dodatkowy crate wymaga uzasadnienia w komentarzu w `Cargo.toml`,
* moduły logiki gry (`hexgrid`, `board`, `entities`, `game`, `ai`, `rng`, `mapfile`) nie zależą od macroquad — dzięki temu testy logiki działają bez okna. Katalog dźwięków (`sound.rs`) też nie zależy od macroquad, więc mapowanie zdarzeń na pliki jest testowalne headless; z backendem rozmawia wyłącznie `audio.rs`.

## Planowany układ kodu
```text
rust/Cargo.toml        manifest crate'a (Cargo.lock wersjonowany; target/ w .gitignore)
rust/src/main.rs       punkt wejścia gry (cargo run --release)
rust/src/constants.rs  stałe gry: UNIT_J_TO_PX jako jedyne miejsce przelicznika j → px, FPS i SIM_DT (prędkość klatki — decyzja tej implementacji), współczynniki rzutu izometrycznego, REPO_ROOT, MAPS_DIR, MAP_EXTENSION; każda stała z komentarzem wskazującym sekcję rules.md lub specification.md
rust/src/hexgrid.rs    geometria sześciokątów flat-top (odd-q) bez logiki gry
rust/src/board.rs      plansza: pola, utrudnienia, podjazdy, mosty, tryb przejazdu przez most, wyszukiwanie drogi, wskazywanie pola kursorem (tryb „płaski” z klawiszem Alt)
rust/src/entities.rs   rodzaje budynków, gracze, budynki, pojazdy i ich pojemności
rust/src/game.rs       symulacja czasu rzeczywistego o stałym kroku SIM_DT, bez zależności od macroquad
rust/src/ai.rs         sterowanie przeciwnikami według sekcji 13 rules.md; deterministyczne dla danego seeda
rust/src/rng.rs        własny deterministyczny PRNG (bez dodatkowych crate'ów, niezależny od macroquad)
rust/src/mapfile.rs    zapis i odczyt formatu z specification_of_map_format.md (tabele kodów) oraz lista poziomów dla menu; używany też przez edytor
rust/src/camera.rs     rzut izometryczny i widok: przesuwanie, zoom, ograniczanie do planszy
rust/src/iso.rs        ortograficzna kamera GPU odtwarzająca rzut 2D (macierze view/proj, głębia z D wzdłuż osi Z)
rust/src/mesh.rs       budowa meshy GPU bez zależności od macroquad: statyczny teren (chunki na u16) i dynamiczne obiekty co klatkę
rust/src/fx.rs         system cząstek wybuchu zniszczonych pojazdów (emittery, krzywe kolorów, symulacja cząstek i budowa ich geometrii); bez zależności od macroquad
rust/src/sound.rs      katalog dźwięków: mapowanie zdarzenia na plik Ogg Vorbis z katalogu sounds/ (wraz z wyborem wariantu); bez zależności od macroquad
rust/src/decode.rs     dekodowanie nagrań (lewton) i korekta próbkowania do 44100 Hz (resampler), wynik jako WAV w pamięci; bez zależności od macroquad
rust/src/audio.rs      wczytywanie i odtwarzanie plików Ogg Vorbis z sounds/ przez backend macroquad: tłumienie odległości, limit głosów na krok i ograniczenie powtórzeń tego samego dźwięku
rust/src/render.rs     rysowanie z kodu na GPU, bez assetów rastrowych (chunki terenu, linie siatki, obiekty, maski i kompozycja zasięgów, kreski 3D, nakładki 2D)
rust/src/render_baseline.rs  test-benchmark budowy meshy (`#[cfg(test)]`, tylko na potrzeby pomiaru)
rust/src/editor.rs     edytor map jako stan aplikacji (nie osobny program): operacje na `Board`, trim/pad, walidacja, pamięć ostatniego budynku/utrudnienia
rust/src/app.rs        okno, menu poziomów, gra i edytor: obsługa wejścia i pętla aplikacji
```

## Uruchamianie i testy
```bash
cd rust && cargo run --release   # gra
cd rust && cargo test            # testy
```

Kod formatowany jest rustfmtem (domyślne ustawienia). Testy logiki działają bez okna, bo moduły logiki nie zależą od macroquad; są to moduły `#[cfg(test)]` wewnątrz modułów logiki. Zakres: reguły z [rules.md](rules.md) (ruch, produkcja, walka, działka, wieże, eliminacja, AI), edytor (trim/pad, walidacja, cykle klawiszy — headless, bez okna) oraz format plansz — odczyt tych samych plików z `maps/` musi dawać ten sam stan początkowy w każdej implementacji, co wynika z formatu ([specification_of_map_format.md](specification_of_map_format.md)), a nie z jednej implementacji; test formatu czyta rzeczywiste pliki z `maps/`.

## Renderowanie

Wspólny kontrakt ([specification.md](specification.md), sekcja „Grafika i interfejs użytkownika") wymaga zasłaniania na poziomie pikseli według głębokości `D = (x + y)·k + z`. **Decyzja: scena jest rysowana jako meshy trójkątów na GPU, a zasłanianie rozstrzyga sprzętowy test głębokości.** Ortograficzna kamera GPU (`iso.rs`) odtwarza rzut 2D z `camera.rs` (pan i zoom to tylko zmiana macierzy — zero pracy CPU na klatkę), a oś głębokości niesie `D`, więc sprzęt rozstrzyga zasłanianie dokładnie tak, jak robił to dawny programowy bufor. Tekst interfejsu (menu, liczniki jednostek, pływające liczby) rysujemy na wierzchu wbudowanym fontem macroquad.

**Kolejność przejść** jest ustalona i wynika z tego, co może zasłaniać co: teren → cienie mostów → linie siatki → obiekty dynamiczne → cienie helikopterów → cząstki wybuchów → kreski 3D → wypełnienia zasięgów → obrysy zasięgów → nakładki 2D → teksty. Remisy współpłaszczyznowych fragmentów rozstrzyga właśnie ta kolejność. Wykonywanie poszczególnych przejść opisuje dokumentacja modułu `render.rs`.

**Zasięgi** rysujemy dwuprzebiegowo i bez testu głębokości: wypełnienia są maskami offscreen komponowanymi po złożeniu sceny, dzięki czemu dwa zasięgi tego samego rodzaju nakładają się bez kumulatora krycia, a obrys rysowany po kompozycji pozostaje czytelny. Kolory i alfy zasięgów są w `constants.rs`.

**Podział na partie** buforów trójkątów robimy na granicach trójkątów, bo limit partii (indeksy `u16`) nie jest wielokrotnością trzech, a cięcie w środku trójkąta zostawiłoby uszkodzony trójkąt na każdym szwie.

**Teren statyczny** budujemy raz na poziom, obiekty dynamiczne co klatkę. Przebudowa terenu następuje tylko przy zmianie planszy (w edytorze: przy każdej zmianie terenu, rampy lub mostu). Jawne odrzucanie prymitywów na CPU nie ma — clipping realizuje sprzęt. Pomiar kosztu budowy meshy daje `rust/src/render_baseline.rs` (test `render_baseline`, `cargo test --release render_baseline -- --nocapture`). Twardych wymagań czasowych nie stawia żadna specyfikacja — pętlę klatki steruje macroquad (`next_frame`).

**Modele obiektów** (budynki, pojazdy, utrudnienia, mosty) są bryłami budowanymi z kodu, bez assetów rastrowych, z rozmiarami w px trzymanymi przy kodzie budowy meshy w `mesh.rs`. Szczegóły każdego modelu, mostów, cieni i cząstek opisuje dokumentacja `mesh.rs` i `fx.rs`; to element wyłącznie tej implementacji. Wygląd celowo nie musi być zgodny piksel w piksel — wiąże kontrakt, nie piksele.

## Dźwięk
Dźwięk jest opisany jako część wspólna kontraktu w [specification.md](specification.md), sekcja „Dźwięk”. W tej implementacji są to **pliki Ogg Vorbis w katalogu `sounds/`** w katalogu głównym repozytorium (obok `maps/`). Ich pochodzenie, licencja i sposób przygotowania są opisane w [`sounds/README.md`](sounds/README.md). Katalog zawiera pliki tylko dla zdarzeń bojowych — muzyki, interfejsu i otoczenia gra nie odtwarza.

**Decyzja: trzy warstwy, każda testowalna bez okna i bez karty dźwiękowej.**

* [`rust/src/sound.rs`](rust/src/sound.rs) — **katalog dźwięków**: które nagranie odpowiada na które zdarzenie, wraz z deterministycznym wyborem wariantu. Testy sprawdzają mapowanie na prawdziwych plikach. Moduł nie zależy od macroquad.
* [`rust/src/decode.rs`](rust/src/decode.rs) — **dekodowanie**: plik jest dekodowany (`lewton`), próbkowanie korygowane do 44100 Hz (`resampler`) i pakowany do WAV-a w pamięci. Nie zależy od macroquad, a testy dekodują wszystkie prawdziwe pliki.
* [`rust/src/audio.rs`](rust/src/audio.rs) — **odtwarzanie**: wczytuje zdekodowane bajty raz na starcie i w każdym kroku symulacji zamienia zdarzenia na głosy — tłumi względem odległości, wydziela limit głosów na krok na najbliższe zdarzenia i ogranicza powtórzenia tego samego dźwięku. To jedyny moduł rozmawiający z backendem audio.

**Dlaczego dekodujemy sami, a nie zostawiamy to backendowi:** pliki trzymamy w oryginalnym kodowaniu (Vorbis jest stratny, a kolejne przekodowanie to strata), a własny resampler backendu to próbkowanie sąsiednich próbek bez filtracji — skraca nagranie i podnosi energię wysokich częstotliwości. Robiąc resampling w `decode.rs`, backend dostaje już 44100 Hz i wadliwą ścieżkę pomija. Próbkowanie i liczba kanałów są brane z nagłówka pliku, nie zakładane, więc nagranie o innym próbkowaniu zadziała bez zmian w kodzie.

**Separacja warstw** jest taka sama jak przy cząstkach: symulacja raportuje **co** się wydarzyło (`SoundEvent` w `Game::sounds`, odbierane przez `Game::take_sounds()`), a warstwa prezentacji decyduje, **jak** to brzmi — dokładnie tak, jak `Wreck`/`take_wrecks()` raportują wybuch. Logika nie wie nic o dźwięku.

**Warianty.** Każde zdarzenie ma kilka nagrań, bo jedno puszczane w kółko od razu daje wrażenie powtórki. Wariant losuje `rng.rs` zasiany seidem poziomu: ten sam poziom zawsze brzmi tak samo, a dwa wybuchy w jednej bitwie się różnią. Wariant jest losowany dopiero dla zdarzeń, które faktycznie zostaną odtworzone.

Brak działającego urządzenia dźwiękowego nie może uniemożliwić uruchomienia gry (maszyna bez karty, kontener, CI), a brak katalogu `sounds/` — tym bardziej, więc nieudane wczytanie wyłącza dźwięk i zapisuje ten stan zamiast panikować. Uzasadnienia poszczególnych wartości i pomiar aliasingu opisuje dokumentacja `decode.rs` i `audio.rs`.

## Struktura aplikacji
Aplikacja ma stany: menu poziomów, gra i edytor plansz (pauza jest flagą stanu gry; w edytorze pauzy nie ma). Okno tworzymy w domyślnej konfiguracji macroquad — nie wpisujemy rozmiaru, więc okno jest skalowalne, a wymiar renderowania pobieramy co klatkę; zmiana rozmiaru okna wymusza przebudowanie buforów rasteryzacji i unieważnia cache terenu. Obsługa wejścia jest opisana w [specification.md](specification.md) (sekcja „Sterowanie") i obowiązuje w grze bez zmian. Szczegóły stanów, HUD-u i menu opisuje dokumentacja `app.rs`. Wejścia do edytora opisuje sekcja „Edytor plansz" poniżej.

## Edytor plansz
Edytor jest integralną częścią tej samej binarki — stan edytora w `app.rs` z logiką w `editor.rs`, a nie osobny program. Wskazywanie pola myszą działa identycznie jak w grze (w tym tryb „płaski” klawiszem `Alt`), a gra i edytor współdzielą renderer i picking (`Renderer::pick_tile()` / `snap_to_building()` tylko delegują do `Board`). Format pliku opisuje [specification_of_map_format.md](specification_of_map_format.md), implementacja jest w `mapfile.rs`. Szczegóły zachowania opisuje dokumentacja `editor.rs`.

### Wejście i wyjście
* Menu główne pokazuje obok siatki poziomów przycisk `add map` (zawsze widoczny): kliknięcie otwiera edytor z nową mapą. Kliknięcie pozycji mapy prawym przyciskiem otwiera tę mapę do edycji; kliknięcie lewym przyciskiem rozpoczyna grę.
* Prawy przycisk ma różne znaczenie w każdym stanie, a stany są rozłączne: w menu — edytuj mapę, w grze — anulowanie zaznaczenia (sekcja „Sterowanie" [specification.md](specification.md)), w edytorze — kasowanie obiektu.
* Wyjście klawiszem `Esc` wraca do menu głównego; przy niezapisanych zmianach edytor pyta o zapis. Zapis nie wychodzi z edytora, a po zapisie lista `maps` w menu jest odświeżana.

### Sterowanie edycją
* `b` stawia budynek albo cyklicznie zmienia rodzaj budynku już stojącego na polu; `o` cyklicznie zmienia właściciela; `t` wstawia utrudnienie lub cyklicznie zmienia jego rodzaj.
* Cyfry zmieniają liczbę jednostek w budynku (0–999). Wpisywanie zatwierdzane jest natychmiast po trzeciej cyfrze albo po chwili od wpisania pierwszej lub drugiej (stała `EDITOR_DIGIT_COMMIT_DELAY`); bez budynku nie zmienia nic, a wpisany ciąg jest związany z polem, na którym zacząto.
* `m` wstawia most lub cyklicznie go obraca; `r` wstawia podjazd (o kierunku między dwoma przeciwległymi sąsiadami o różnych wysokościach, jeśli istnieją) lub cyklicznie go obraca. Edycja zawsze dotyczy **jednego pola** (znaku osi tego pola), a mosty są przebudowywane ze wszystkich znaków, więc obrót jednego fragmentu nie kasuje reszty wielopolowego mostu, a usunięcie fragmentu zostawia pozostałe. Wstawianie fragmentu nie zmienia wysokości pola — zbyt wysokie albo zbyt niskie pole zgłasza walidacja. Gra testowa używa `p`, więc `r` jest tu wolny.
* `[` / `]` obniżają / podwyższają teren o 1 (nic przy granicy wysokości). `Del` lub prawy przycisk kasuje obiekt; wstawienie obiektu nadpisuje obiekt poprzednio stojący na polu.
* `l` ładuje mapę z pliku, `s` zapisuje mapę (wpisanie nazwy albo wybór istniejącej), `ctrl`+`s` zapisuje pod wcześniej wybraną nazwą, `ctrl`+`n` tworzy nową mapę.
* Nakładki list (`l`/`s`) i wpisywanie nazwy realizowane są w tym samym oknie — nie ma osobnego okna dialogowego. Klawisze literowe to fizyczne kody macroquad/miniquad, więc na innych układach klawiatury pozycje mogą się różnić.
* Pierwszy wstawiony budynek to neutralna baza czołgowa o zerze jednostek; ostatnio użyte właściwości budynków (typ, właściciel, liczba jednostek) i rodzaj utrudnienia są zapamiętywane i nadawane nowo wstawianym obiektom.
* Działka i wieże lecznicze wyświetlane są wraz z zasięgiem (zgodnie z regułami gry, zależnie od liczby jednostek; neutralne wieże lecznicze bez zasięgu). Edytor wyświetla legendę klawiszy, a przy niezgodności z regułami gry wypisuje błędy na czerwono — zapis mapy z błędami jest możliwy.

### Gra testowa na edytowanym poziomie
* `p` uruchamia grę na edytowanej poziomie, zbudowaną z kopii planszy, więc test nigdy nie zmienia edytowanej mapy: gracze wynikają z postawionych właścicieli (właściciel 0 sterowany ręcznie, pozostali przez AI), a seed AI wyznaczany jest od nazwy pliku mapy tak jak dla poziomu.
* Gra testowa jest **piaskownicą**: mecz nigdy się nie kończy (warunki zwycięstwa i eliminacji z sekcji 2 [rules.md](rules.md) nie obowiązują), więc można testować mapy bez bazy przeciwnika; AI działa normalnie.
* `Esc` w grze testowej wraca do edytora (najpierw anuluje zaznaczenie, tak jak w normalnej grze), a stan edytora, mapa i kamera zostają nienaruszone. Wyjście do menu głównego z gry testowej jest niemożliwe. Zwykła gra z menu zachowuje dotychczasowe wyjście klawiszem `Esc`.

### Widok i rozmiar planszy
Widok i jego sterowanie są takie jak w grze, poza kolidującymi cechami: bez `WASD` (kolizja klawisza `s`) i bez panoramowania prawym przyciskiem (bo kasuje obiekt). Stałe edytora (`EDITOR_*`) mieszkają w `constants.rs`.

Nowo utworzona plansza ma wymiary podane w `EDITOR_NEW_COLS` × `EDITOR_NEW_ROWS` i w większości składa się z wody; na środku prostokąt lądu o wysokości 1, widok wycentrowany. Przy zapisie puste (sama woda bez obiektów) początkowe oraz końcowe wiersze i kolumny są usuwane, a przy odczycie plansze mniejsze od nowej są poszerzane o wiersze i kolumny po równo na początku/końcu. Ze względu na geometrię siatki heksów przesunięcie kolumnowe przy usuwaniu i dopełnianiu jest zawsze parzyste — opisuje to dokumentacja `trim_map()` i `pad_map()` w `editor.rs`.

## Determinizm i czas
Prędkość klatki jest decyzją tej implementacji: `FPS` i `SIM_DT` to stałe w `constants.rs`, a symulacja działa o stałym kroku `SIM_DT` niezależnie od zegara systemowego — czas rzeczywisty służy wyłącznie do zliczania kroków. Symulacja i AI są deterministyczne dla danego seeda (sekcja 13 [rules.md](rules.md)); seed wyznaczany jest z nazwy pliku poziomu. Determinizm jest wymagany wewnątrz tej implementacji. Losowość (szum AI, warianty dźwięków, cząstki) realizuje `rng.rs` własnym, deterministycznym generatorem, bez dodatkowych crate'ów; strumienie są zasiany seidem poziomu przy starcie meczu i gry testowej.

## Katalog maps
Ścieżkę katalogu `maps` wyznaczamy względem katalogu głównego repozytorium, a nie względem katalogu roboczego — to zasada z [specification.md](specification.md) (sekcja „Struktura repozytorium”). Menu poziomów listuje wszystkie pliki `*.map` z tego katalogu (nazwa pliku jest nazwą poziomu), bez hardcodowania listy. Sposób odnajdywania katalogu głównego opisuje dokumentacja `constants.rs`.
