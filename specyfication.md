# War Regions — specyfikacja implementacji gry

## Wstęp
War Regions to gra komputerowa napisana w Pythonie, przy użyciu bibliotek PyGame i PyTMX.

Reguły gry znajdują się w pliku [rules.md](rules.md).

## Kod
Kod jest przejrzysty i dobrze udokumentowany, w języku angielskim.
Wszelkie funkcje, metody, klasy, pola, itp. mają dokumentację.
Logika gry jest sensownie oddzielona i niezależna od interfejsu użytkownika (tą regułe może być nagięta w uzasadnionych przypadkach).
Wszelkie stałe są zdefiniowane (najlepiej w osobnym pliku/plikach) i udokumentowane, także można je łatwo zmienić i eksperymentować z innymi wartościami. Stałe dotyczące odległości, zasięgów i prędkości wyrażone są w jednostkach odległości (j) z rules.md; przelicznik j → piksele zdefiniowany jest w jednym miejscu.

## Grafika i interfejs użytkownika
Grafika jest izometryczna. Plansza rysuje się z kodu. Okno gry można skalować.
Obiekty także rysuje się z kodu (w przyszłości możliwe podmienienie na grafikę rastrową).
Obiekty poszczególnych graczy różnią się kolorem (kod rysujący przyjmuje kolor jako argument).

Przy budynkach i pojazdach, po prawej stronie z dołu wyświetla się kółko z białą liczbą po środku oznaczającą ilość jednostek w budynku lub pojeździe. Jeśli w budynku jest przynajmniej maksymalna ilość jednostek, pod liczbą wyświetla się napis 'MAX', który nadal mieści się w kółku. Jeśli w bazie rodzą się jednostki, jest renderowany biały okrąg dopełniający się wraz z czasem, który nie nachodzi nigdy na numer w tym kółku. Jeśli okrąg się dopełni do końca, w bazie rodzą się jednostki. Ląd ma kolor szary, a woda jasny niebieski.

Po wysłaniu pojazdu jego droga rysowana jest przerywaną kreską, która znika za pojazdem.
Gdy pojazd lub budynek traci x jednostek, to wyświetla się biała liczba -x lecąca przez 2 sekundy do góry od liczby oznaczającej ilość jednostek w tym pojeździe lub budynku. Wyjątek stanowi wysyłanie pojazdu z budynku, wtedy taka liczba się nie wyświetla. Gdy pojazd zyskuje x jednostek z powodu leczenia, wyświetla się jasnozielone +x.

Zasięgi działek są renderowane jako białe, a wież leczących jako jasnozielone. Zasięgi mają spory procent przezroczystości.

Po uruchomieniu gry wyświetla się menu, z poziomami: każdy poziom ma swoją wyświetlaną nazwę.
Po wybraniu poziomu on się ładuje pokazując mapę.


## Dźwięk
Brak dźwięku (w przyszłości to się może zmienić).

## Parametry liczbowe
Wszystkie odległości, promienie i zasięgi w grze wyrażone są w jednostkach odległości (j) zdefiniowanych w rules.md. Przyjmuje się przelicznik **1 j = 1 px** przy skali widoku 1:1; zoom i skalowanie okna dotyczą wyłącznie renderingu. Przelicznik jest jedną stałą w kodzie — jego zmiana (np. na 1 j = 2 px dla wyświetlaczy o dużej gęstości) nie wpływa na logikę gry.

### Geometria planszy

* bok sześciokąta: **36 j** (= 36 px przy skali 1:1; układ flat-top),
* szerokość pola: **72 j** (= 72 px), wysokość pola: **≈62,4 j** (≈62,4 px; √3 × 36),
* odległość między środkami sąsiednich pól: **≈62,4 j** — umowna jednostka „1 heksa odległości", wygodna do wyrażania zasięgów.

### Wykrywanie

* promień wykrywania pojazdów: **80 j** (= 80 px; koło; obejmuje 6 sąsiednich pól, bez drugiego pierścienia).

### Zasięgi i prędkości (wartości domyślne)

* promień leczenia bufora: **160 j** (§5.4),
* zasięg zwykłego działka: **250 j**, szybkostrzelnego: **190 j**, rakietowego: **375 j**; promień splashu rakietówki: **160 j** (§10),
* zasięg wieży leczniczej: **80 j + x j**, gdzie x — ilość jednostek w wieży (§11),
* prędkości pojazdów: czołg **60 j/s**, bufor **60 j/s**, poduszkowiec **48 j/s**, helikopter **90 j/s** (§5); pułapka lodowa — połowa prędkości,
* częstotliwość decyzji AI: **co 5 s**.

### Do ustalenia

Poniższe elementy nie są jeszcze zaprojektowane:

* szczegółowa strategia AI (poza częstotliwością decyzji),
* konstrukcja map/poziomów (format, niezmienniki mapy).
