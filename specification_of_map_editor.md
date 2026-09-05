# War Regions — specyfikacja implementacji gry

## Wstęp
Ten plik uzupełnia specyfikację [specification.md](specification.md), o szczegóły edytora map do gry o zasadach opisanych w [rules.md](rules.md).

Edytor dzieli kod rysujący planszę z grą. Plansze zapisywane są w katalogu maps w plikach o rozszerzeniu `map`, każda w osobnym pliku.

## Sterowanie i interfejs użytkownika
Edycja polega na wskazaniu pola za pomocą myszy (jest ono wskazywane dokładnie tak jak w samej grze) i następnie wciśnięciu jednego z klawiszy, który zmienia właściwości pola albo obiektu na nim stojącego.

Działanie poszczególnych klawiszy:
* `b` stawia budynek albo, jeśli na polu już znajduje się budynek, cyklicznie zmienia jego rodzaj;
* cyfry - zmieniają liczbę jednostek (w zakresie 0-255) w budynku (wpisywanie jest zatwierdzanie natychmiast po wpisaniu trzeciej cyfry albo chwilę po wpisaniu pierwszej lub drugiej cyfry);
* `o` cyklicznie zmienia własciciela budynku (brak działania jeśli na polu nie ma budynku);
* `t` wstawia utrudnienie lub cyklicznie zmienia jego rodzaj;
* `m` wstawia most lub cyklicznie go obraca;
* `r` wstawia podjazd lub cyklicznie go obraca;
* `[` zmniejsza wysokość terenu o 1 (modulo 16);
* `]` zwiększa wysokość terenu o 1 (modulo 16);
* `Del` lub prawy przycisk myszy - kasuje obiekt.
* `l` ładuje mapę z pliku (wyświetla nazwy z katalogu maps do wskazania);
* `s` zapisuje mapę do pliku (można podać nazwę albo wybrać jedną z istniejących; nazwa wybrana dla tej mapy wyświetla się jako pierwsza i jest wyróżniona).

Wstawienie obiektu nadpisuje obiekt który znajdował się na polu wcześniej.

Edytor wyświetla legendę z opisem działania klawiszy.