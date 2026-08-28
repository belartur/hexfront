# War Regions — specyfikacja implementacji gry

## Wstęp
War Regions to gra komputerowa napisana w Pythonie, przy użyciu bibliotek PyGame i PyTMX.

Reguły gry znajdują się w pliku [rules.md](rules.md).

## Kod
Kod jest przejrzysty i dobrze udokumentowany, w języku angielskim.
Wszelkie funkcje, metody, klasy, pola, itp. mają dokumentację.
Logika gry jest sensownie oddzielona i niezależna od interfejsu użytkownika (tą regułe może być nagięta w uzasadnionych przypadkach).
Wszelkie stałe są zdefiniowane (najlepiej w osobnym pliku/plikach) i udokumentowane, także można je łatwo zmienić i eksperymentować z innymi wartościami.

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