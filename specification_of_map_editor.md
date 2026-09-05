# War Regions — specyfikacja implementacji gry

## Wstęp
Ten plik uzupełnia specyfikację [specification.md](specification.md), o szczegóły edytora map do gry o zasadach opisanych w [rules.md](rules.md).

Edytor dzieli kod rysujący planszę z grą. Plansze zapisywane są w katalogu maps w plikach o rozszerzeniu `map`, każda w osobnym pliku.

## Sterowanie i interfejs użytkownika
Edycja polega na wskazaniu pola za pomocą myszy (jest ono wskazywane dokładnie tak jak w samej grze) i następnie wciśnięciu jednego z klawiszy, który zmienia właściwości pola albo obiektu na nim stojącego.

Działanie poszczególnych klawiszy:
* `b` stawia budynek albo, jeśli na polu już znajduje się budynek, cyklicznie zmienia jego rodzaj (domyślne właściwości nowego budynku:
neutralna baza czołgowa z zerem jednostek);
* cyfry - zmieniają liczbę jednostek (w zakresie 0-255) w budynku (wpisywanie jest zatwierdzanie natychmiast po wpisaniu trzeciej cyfry albo sekundę po wpisaniu pierwszej lub drugiej cyfry; wpisywane cyfry wyświetlane są od razu w polu wyświetlającym liczbę jednostek w budynku; wartość większa od 255 jest ograniczana do 255 w momencie zatwierdzenia; jeśli na polu nie ma budynku, wpisywanie cyfr niczego nie zmienia; wpisywany ciąg cyfr jest związany z polem, na którym rozpoczęto wpisywanie - samo wskazanie innego pola go nie przerywa (po sekundzie zatwierdzi się na starym polu), a wpisanie cyfry na innym polu porzuca stary ciąg i zaczyna nowy dla tego pola);
* `o` cyklicznie zmienia własciciela budynku (brak działania jeśli na polu nie ma budynku);
* `t` wstawia utrudnienie lub cyklicznie zmienia jego rodzaj;
* `m` wstawia most lub cyklicznie go obraca (jeśli na polu sąsiadującym jest most skierowany w stronę bieżącego pola, to wstawianemu mostowi jest nadawany ten sam kierunek, w przeciwnym razie obraca most tak by łaczył dwóch przeciwległych sąsiadów o tej samej wysokości, możliwe najwyższych);
* `r` wstawia podjazd (o kierunku między dwoma przeciwległymi sąsiadami o różnych wysokościach, jeśli istnieją) lub cyklicznie go obraca;
* `[` zmniejsza wysokość terenu o 1 (modulo 16);
* `]` zwiększa wysokość terenu o 1 (modulo 16);
* `Del` lub prawy przycisk myszy - kasuje obiekt.
* `l` ładuje mapę z pliku (wyświetla nazwy z katalogu maps do wskazania);
* `s` zapisuje mapę do pliku (można podać nazwę albo wybrać jedną z istniejących; jeśli mapie nadano już nazwę wcześniej, to wyświetla się ona jako pierwsza i jest wyróżniona);
* `ctrl`+`n` czyszczenie / tworzenie nowej mapy (bez żądania potwierdzenia).

Wstawienie obiektu nadpisuje obiekt który znajdował się na polu wcześniej.

Edytor wyświetla legendę z opisem działania klawiszy.

Działka i wieże lecznicze są wyświetlane wraz z zasięgiem (w przypadku wież leczniczych zgodnie z regułami gry, czyli zależnie od liczby jednostek; neutralne wieże lecznicze nie mają wyświetlanego zasięgu).

Przy wychodzeniu edytor pyta czy zapisać zmiany (jeśli nie są już zapisane).

Jeśli mapa jest niezgodna z regułami gry to na ekranie wypisywane są błędy (czerwonym kolorem):
* budynek na wodzie,
* most nad za wysokim lądem,
* most łączący dwa pola o różnej wysokości,
* podjazd na polu o innej wysokości niż niższe z łączonych pól,
* podjazd łączący pola o tych samych wysokościach,
* brak bazy gracza,
* brak bazy przynajmniej jednego przeciwnika.
Wciąż można zapisać (i potem wczytać) mapę z błędami.

Widok i jego sterowanie jest taki jak w samej grze, poza kolidującymi cechami (np. bez WASD, bo koliduje klawisz `s` i bez panoramowania PPM).

Nowo utworzona plansza ma wymiary 100 na 100 i w większości składa się z wody. Na jej środku jest prostokąt 20 na 13 o wysokości 1. Widok jest centrowany na ten prostokąt. Przy zapisie, puste (sama woda bez obiektów) początkowe oraz końcowe wiersze i kolumny są usuwane (minimalny rozmiar zapisanej mapy to 1 na 1). Przy odczycie plansz o wymiarach poniżej 100 na 100 są one poszerzane (dopełniane do 100 na 100) o dodatkowe wiersze i kolumny (po równo na początku/końcu); widok także jest centrowany.