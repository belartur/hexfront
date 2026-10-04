premie.md to plik tymczasowy i po ustaleniu szczegułów informacje mają być przeniesione do innych plików.

premie są elementem mapy, mogą znajdywać się tylko na lądzie. zajmują całe pole, więc na polu z premią nie może się znajdować nic innego — ani budynku, ani utrudnienia, podjazdu czy mostu. same nie są przeszkodą: pojazdy przejeżdżają przez takie pole normalnie, ale go nie zbierają, bo premię wyzwala wyłącznie pojazd, który do niej wysłano.

można do nich wysyłać pojazdy. przy tej misji pojazd porusza się do premii, lecz po osiągnięciu celu dzieje się jej efekt, a następnie pojazdy wracają tą samą drogą do budynków, z których wyruszyły. dojazd powrotny rozlicza się tak jak zwykły dojazd pojazdu, więc jeśli budynek źródłowy w międzyczasie zmieni właściciela, pojazd może go odzyskać. Każdy gracz może aktywować premię.

każda premia jest jednorazowa. pierwszy pojazd, jaki do niej dotrze, po wykonaniu efektu, powoduje jej zniknięcie. Lecz gdy premia zniknie pojazdy do niej wysłane kontynuują swoją podróż i po dotarciu do miejsca, na którym była premia, wracają z niczym — z tymi samymi jednostkami, które mają w drodze, bo mogły je stracić w walce, na minie albo pułapce albo odzyskać dzięki leczeniu. premię wyzwala pojazd, który do niej dotrze i w tej chwili żyje: zniszczony w drodze albo na samym polu premii nie wyzwala jej, bo premia czeka wtedy na kolejnego. efekt działa dopiero wtedy, gdy środek grafiki pojazdu znajduje się na polu premii, więc pojazdy w trakcie walki jej nie wyzwalają. jeżeli w tym samym kroku symulacji na polu premii stoją dwa pojazdy, premię wyzwala ten o mniejszym identyfikatorze pojazdu (tak samo rozstrzyga się remisy celu działka).

każda premia ma dokładnie jeden z efektów:
- +x jednostek
- *x jednostek
- dron

+x i *x jednostek zmieniają siłę pojazdu, czyli liczbę jednostek w nim: +x dodaje x jednostek, a *x mnoży aktualną liczbę jednostek przez x. efekt dotyczy wyłącznie pojazdu, który premię wyzwolił, i nie zmienia liczby jednostek w żadnym budynku. zasięg wartości na mapie: +x od 1 do 999 (tak jak początkowa liczba jednostek w budynkach), *x od 2 do 99.

dron jest jednostką strzelającą. zawsze, jeśli nie znajduje się w premii, jest przywiązany do budynku albo do pojazdu. Strzela tylko gdy nie znajduje się w premii. właścicielem dronu jest właściciel pojazdu/budynku do którego dron jest przywiązany. Jeśli znajduje się w premii zawsze jest neutralny. Aktywacja premii z dronem przywiązuje go do pojazdu aktywującego. Jeśli pojazd z dronem zostanie zniszczony to dron wraca na miejsce swojej premii i zaczyna ją tworzyć ponownie. Jeśli pojazd z dronem dotrze do budynku z którego startował, to dron się przywiązuje do tego budynku. Jeżeli budynek, do którego przywiązany jest dron, zmieni właściciela, to dron też zmienia właściciela. Drona przywiązanego do budynku nic nie może ruszyć. Dron jest niezniszczalny, może jedynie zmieniać właściciela. Dron nie wpływa na wykrywanie ani walkę pojazdów. Dron nie jest ani pojazdem, ani budynkiem, więc nie blokuje eliminacji gracza i nie ma wpływu na zwycięstwo. Dron strzela z stałą prędkością 3 razy na sekundę w stałym zasięgu 80j i zadaje stałe jedno obrażenie. Nie ma on przypisanej liczby jednostek. W przeciwieństwie do działek efekt pocisków drona rozstrzygany jest natychmiastowo. Do jednego budynku można przypisać dowolnie dużo dronów i wtedy wszystkie mają działanie (tj. każdy dron działa niezależnie).

AI uwzględnia premie jako dodatkowe cele wysyłki — wysłanie pojazdu do premii jest tą samą akcją co wysłanie go do budynku, tylko o innym celu. W ramach jednej decyzji AI wykonuje co najwyżej jedną akcję, więc premia jest tylko jednym z kandydatów obok par budynków.

AI uwzględnia położenie, rodzaj i wartość każdej premii, która jeszcze istnieje w meczu.

Punktacja premii nie używa szansy przejęcia ani wartości budynku, bo premia nie ma załogi ani właściciela. Zamiast tego liczy się wartość jej efektu: premia +x jest warta tyle, ile wynosi x podzielone przez liczbę jednostek w pojeździe; premia *x — przyrost z mnożnika, czyli x−1; premia z dronem ma wartość stałą, niezależną od liczby jednostek. Mnożnik działa na cały oddział, więc premie *x mają z natury większą wartość niż premie +x o tej samej wartości liczbowej — autor mapy sam ustawia je na mapie.

Misja po premię jest podróżą w obie strony: pojazd wraca tą samą drogą, którą przyszedł. Dlatego koszt misji jest podwójny — czas podróży, niebezpieczeństwo trasy i oczekiwane obrażenia na drodze powrotnej są takie same jak na drodze tam, a więc liczone dwa razy. Podwojenie jest oszacowaniem zachowawczym, bo wyjście może zniszczyć ściany i miny na trasie, więc powrót jest odrobinę bezpieczniejszy.

Wartość premii maleje o proporcję jednostek, o jakie zmniejsza się oczekiwana liczba jednostek w pojeździe po powrocie (dla premii +x i *x). Dla premii z dronem czynnik ten nie obowiązuje, bo dron po zniszczeniu pojazdu wraca na swoją premię i odtwarza ją.

AI nie odrzuca premii tylko dlatego, że po nią jedzie pojazd przeciwnika — zdążyć może również sam, wygrywając walkę w drodze, bo walka nie zmienia celu pojazdu. Dlatego jeżeli do tej samej premii jedzie pojazd przeciwnika, to wartość premii jest pomniejszona o szansę zdobycia jej: jest pełna, gdy własny pojazd dotrze nie później, a gdy dotrze później, liczona jest tylko możliwość wygrania walki z pojazdem przeciwnika — pełna przy przewadze jednostek, zerowa przy jej braku. AI nie wysyła drugiego pojazdu po tę samą premię, po której jedzie już jego własny pojazd.

AI nie wysyła pojazdu po premię, gdy oczekiwane obrażenia na trasie są nie mniejsze niż liczba jednostek w pojeździe. AI nie wysyła też pojazdu po premię z budynku zagrożonego, jeżeli nie ma w nim innej własnej siły, która budynek broni — misja po premię pozostawia budynek pusty na dwa razy dłużej. Próg wysyłki jest wspólny dla premii i dla budynków.

Premie są oznaczane poprzez efekt w żółtej otoczce, przy czym otoczka to obrys całego pola, a symbol efektu rysowany jest wewnątrz niej. Jeśli efekt to +x jednostek lub *x jednostek to rysuje się ten napis oraz parę ludzików. Jeśli efektem jest dron to rysuje się on. Dron jest małym obiektem, znacznie mniejszym od pojazdów. O ile w premii się nie rusza, o tyle gdy jest do czegoś przypisany to ma animację orbitowania wokół tego, co nie wpływa na zasięg, który jest liczony z środka tego budynku/pojazdu. gdy jest przypisany do budynku/pojazdu a i strzela do pojazdu b to pojawia się na linii między środkami a i b.

W edytorze stawia się premię za pomocą klawisza `i` (od „item”, czyli coś, co można podnieść). następne kliknięcia tego klawisza zmieniają typ premii w kolejności +x, *x i dron. ilość (w +x i *x) wyznacza się cyframi tak jak początkową ilość jednostek w budynkach, w zakresie obowiązującym dla danego efektu (1–999 dla +x, 2–99 dla *x) (patrz: [specification_of_map_format.md](specification_of_map_format.md)). nowo stawiana premia ma wartość +10 albo *2, a ostatnio wpisana ilość jest zapamiętywana i nadawana kolejnym premiom, tak jak pozostałe właściwości obiektów. edytor zgłasza premię, do której nie prowadzi droga z żadnego budynku — taka premia jest nieosiągalna dla każdego pojazdu (ostrzeżenie, jak pozostałe, bez blokowania zapisu).

format zapisu premii: premia ma jeden kod typu 32, a po nim słowo 16-bitowe little-endian:

* bity 15–14 (dwa najwyższe bity pierwszego bajtu) — rodzaj premii: 0 = `+x`, 1 = `*x`, 2 = dron,
* bity 7–0 (drugi bajt) — wartość efektu: dla `+x` liczba jednostek (1–999, tak jak początkowe jednostki w budynkach), dla `*x` mnożnik — zakres 2–99 mieści się w tym jednym bajcie w całości,
* pozostałe bity pierwszego bajtu (13–8) — rezerwowe, zapisywane jako zero.

Rekord premii ma zawsze 5 bajtów: 2 bajty pozycji pola, 1 bajt kodu typu i 2 bajty słowa. Wartość spoza zakresu (także 0 dla `+x`) czytnik sprowadza do najbliższej dopuszczalnej wartości z ostrzeżeniem.