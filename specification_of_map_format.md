# Hexfront — format pliku planszy

## Wstęp
Ten plik opisuje binarny format pliku planszy (poziomu). Format jest wspólny dla wszystkich implementacji gry: zasady gry znajdują się w [rules.md](rules.md), specyfikacje poszczególnych implementacji (dla Pythona [specification.md](specification.md)), a zachowanie edytora plansz w [specification_of_map_editor.md](specification_of_map_editor.md).

Jedna plansza zapisywana jest w jednym pliku o rozszerzeniu `.map`; wyświetlaną nazwą poziomu jest nazwa pliku bez rozszerzenia. Plik nie zawiera bajtów identyfikujących (magic) ani sumy kontrolnej — pierwsze dwa bajty to już wymiary planszy. Wszystkie liczby wielobajtowe zapisane są little-endian (młodszy bajt jako pierwszy w pliku, młodsza wartość na młodszych bitach).

## Geometria planszy
Położenie pola opisuje para liczb: kolumna `q` i wiersz `r` (indeksy liczone od zera). Siatka jest typu flat-top w wariancie *odd-q*: nieparzyste kolumny są przesunięte o pół heksu w dół.

Kierunki ruchu numerowane są 0–5 w kolejności kątów 30°, 90°, 150°, 210°, 270°, 330° (oś `y` ekranu rośnie w dół). Sąsiedzi pola `(q, r)` mają współrzędne:

| kierunek | kolumna `q` parzysta | kolumna `q` nieparzysta |
|---|---|---|
| 0 | `(q+1, r)` | `(q+1, r+1)` |
| 1 | `(q, r+1)` | `(q, r+1)` |
| 2 | `(q−1, r)` | `(q−1, r+1)` |
| 3 | `(q−1, r−1)` | `(q−1, r)` |
| 4 | `(q, r−1)` | `(q, r−1)` |
| 5 | `(q+1, r−1)` | `(q+1, r)` |

Kierunkiem przeciwnym do `d` jest `(d + 3) mod 6`, a kolejne kroki w tym samym kierunku prowadzą przez pola leżące w jednej linii prostej (korytarz heksów). **Osią** nazywamy parę przeciwległych kierunków `{a, a+3}`, gdzie `a ∈ {0, 1, 2}`, czyli kierunek modulo 3. Oś kodują mosty i podjazdy.

Wysokość pola jest liczbą całkowitą z zakresu 0–15; 0 oznacza wodę, a większa wartość — ląd ([rules.md](rules.md), sekcja 1).

## Układ pliku
Znajdują się w nim, kolejno, następujące informacje:

* Wymiary planszy (2 bajty): liczba kolumn `k` (1 bajt) i wierszy `w` (1 bajt), każda z zakresu 1–255 (maksimum wynika z 1 bajta na wymiar; `0` jest odrzucane przy odczycie jako pusta plansza).
* `k·w` liczb 4-bitowych kodujących wysokości kolejnych pól planszy (0–15, 0 to woda) w kolejności row-major: indeks `r·k+q`. Zapisane są na `⌈k·w/2⌉` bajtach, po dwa pola na bajt: wcześniejsze pole pary na młodszych 4 bitach (bity 3–0), późniejsze na starszych 4 bitach (bity 7–4). Gdy `k·w` jest nieparzyste, starsze 4 bity ostatniego bajta przechowują zero (padding).
* Obiekty znajdujące się na planszy, jeden rekord za drugim aż do końca pliku (bez licznika ani terminatora); kolejność rekordów jest dowolna. Liczba użytych bajtów zależy od typu obiektu: pierwsze 2 bajty kodują położenie obiektu (1 bajt kolumnę `q` i 1 bajt wiersz `r`), trzeci bajt koduje typ obiektu, a kolejne 2 bajty (wyłącznie w przypadku budynków) jego własności. Na jednym polu może znajdować się najwyżej jeden obiekt.

## Rekordy obiektów

### Budynki (typy 0–19)
| typ | rodzaj budynku |
|---|---|
| 0 | baza czołgowa |
| 1 | baza helikopterowa |
| 2 | baza poduszkowcowa |
| 3 | baza buforowa |
| 4 | działko zwykłe |
| 5 | działko szybkostrzelne |
| 6 | działko rakietowe |
| 7 | wieża lecznicza |
| 8–19 | zarezerwowane (nieużywane) |

Rodzaje budynków opisuje [rules.md](rules.md), sekcja 3. Po typie zapisywane są 2 dodatkowe bajty: jedno 16-bitowe słowo little-endian, w którym 6 starszych bitów zajmuje numer właściciela, a 10 młodszych — początkowa liczba jednostek w budynku (pierwszy z tych bajtów w pliku to bity 7–0, drugi to bity 15–8):

* **właściciel** — 6 starszych bitów (bity 15–10): 0 — neutralny, 1 — niebieski, 2 — czerwony, 3 — zielony, 4 — żółty; wartości 5–63 są rezerwowe,
* **jednostki** — 10 młodszych bitów (bity 9–0): początkowa liczba jednostek w budynku. Zakres przewidziany dla mapy to 0–999; 10 bitów mieści fizycznie 0–1023, więc wartości 1000–1023 są przy odczycie sprowadzane z ostrzeżeniem do 999.

### Mosty (typy 20–22)
`typ − 20` to oś geometryczna mostu. Cały most zapisywany jest jako jeden rekord na każdy fragment pokładu; same końce mostu (pola, które łączy) nie mają osobnych rekordów. Przy odczycie fragmenty o tej samej osi, leżące kolejno na polach sąsiadujących wzdłuż tej osi, składają się w jeden most; jego końcami są pola sąsiadujące z pierwszym i ostatnim fragmentem ciągu od strony zewnętrznej. Most jest poprawny tylko wtedy, gdy spełnia reguły z [rules.md](rules.md), sekcja 8 (m.in. oba końce leżą na tej samej wysokości); ciągi niepoprawne są przy odczycie pomijane (implementacja edytora może zachować je do podglądu — patrz [specification_of_map_editor.md](specification_of_map_editor.md)).

### Podjazdy (typy 23–25)
`typ − 23` to oś pary łączonych przeciwległych sąsiadów. Podjazd na polu `p` łączy pole `a` — sąsiada w kierunku `oś` — z polem `b` — sąsiadem w kierunku `oś+3` ([rules.md](rules.md), sekcja 7). Rekord, którego końce wypadają poza planszę, jest przy odczycie pomijany z ostrzeżeniem.

### Utrudnienia (typy 26 i wyższe)
| typ | rodzaj utrudnienia |
|---|---|
| 26 | ściana |
| 27 | mina lądowa |
| 28 | mina wodna |
| 29 | pułapka ogniowa |
| 30 | pułapka lodowa |
| 31 i wyższe | zarezerwowane (nieznany typ jest przy odczycie pomijany z ostrzeżeniem) |

Utrudnienia opisuje [rules.md](rules.md), sekcja 1.

## Wymagania dla zapisującego i czytnika
Zapisujący:
* plansza większa niż 255 na 255 nie ma reprezentacji w tym formacie (każdy wymiar musi zmieścić się w 1 bajcie),
* wysokość pola jest zapisywana w zakresie 0–15, liczba jednostek w zakresie 0–999, a numer właściciela na 6 bitach,
* budynek o typie spoza tabeli nie jest zapisywany; na jednym polu znajduje się najwyżej jeden obiekt (budynek, podjazd, fragment mostu albo utrudnienie).

Czytnik:
* błąd odczytu powodują: plik krótszy niż 2 bajty, plansza o wymiarze 0, brak pełnych danych wysokości, rekord wskazujący pole poza planszą oraz obcięty rekord budynku (brak 2 bajtów własności),
* ostrzeżenie i korektę danych powodują: liczba jednostek 1000–1023 (sprowadzana do 999), budynek o typie z zakresu zarezerwowanego oraz nieznany typ utrudnienia (rekord pomijany),
* rekord obiektu, którego nie można postawić na danym polu według reguł [rules.md](rules.md), sekcja 1, jest pomijany z ostrzeżeniem.

## Zobacz też
* [rules.md](rules.md) — zasady gry (mapa, budynki, podjazdy, mosty),
* [specification.md](specification.md) — specyfikacja implementacji w Pythonie,
* [specification_of_map_editor.md](specification_of_map_editor.md) — edytor plansz (m.in. usuwanie i dopełnianie pustych skrajnych wierszy i kolumn).
