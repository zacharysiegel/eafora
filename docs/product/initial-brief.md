i want to create an interactive world map to visualize TFR, Effective TFR, Completed FR, etc. for normal people (to help build awareness of the global fertility crisis and help
dispel certain myths through education).

i dont have a name, although i dont think its that imporant. i will need you to help me brainstorm.

technical notes:

i want to use this as an opportunity to practice multiplatform application development. in all seriousness, we really just need a web app, but im working on a separate project (a
game, here: github.com/zacharysiegel/singularity) which i am currently building as a native macos application for this game, but really it is more natural as a mobile game, so i
need to make at least an iphone app but also an android app. even though it's not necessary for this project, i want to make a mobile app for either platform (i have an iphone 17
and a samsung s25 at my disposal. I also work for apple, but idk if thats really relevant). this will help me to figure out how to bridge the platforms for my much more ambitious
game project.

I would like to use rust as much as practical (my game, both frontend and backend are in rust). maybe a wasm toolchain for at least part of the frontend (i have never worked with
wasm before but i really want to finally figure out how to work with it.), but i am curious to hear your input. we probably need a database to aggregate data and not spam public
apis (e.g. world bank), but hypothetically its possible this could be a static application with data shipped with the binary/package (i would like to provide a better service than
that, im just thinking out loud here).

I dont yet know which data sets we should use. ive seen the world bank has a json api which looks very promising, but at the very least its latest data is 2024. i would like to be
able to (at least eventually) merge several data inputs together so we can have a very rich user experience and have the most up to date data. i envision this being *the* leading
data aggregator and visualizer for global fertility data.

i want to hear your input especially on frontend design differences between platforms and best practices to help manage the multiplatform fanout. i want to especially be able to
apply these principles as closely as possible to my game which is vastly more complicated than this project, so i am more than willing to accept more complexity here than would
normally be appropriate in order to learn for my next, more difficult iteration of these tasks. if i make mistakes, i want to make them here, not there.

ui notes:

the main view is just a world map. i like map projection with the humps. we dont care about terrain or weather or starry skies, etc. we care about political divisions, or any
division which might cause segregation in the source data (e.g. if we can get provincial data, we would like to be able to toggle a provincial view, zooming into that country).
users would expect a basic but continuous color code based on TFR (red = low fertility, blue = high). when user's mouse (doesnt make sense on mobile, i know) hovers over a
political area, that area should scale somewhat (smoothed animation (start fast, end slower)). many applications make the mistake of also scaling the input detection zone
alongside the visual indicator, but this is bad ux because it impedes input detection for neighbors. we must avoid this anti-pattern. we of course should draw borders along
political boundaries, but they should be thin. when a user primary clicks a country (or taps) we should auto-zoom on that country and open a detail view about it. for mvp the
detail view can simply show the most recent TFR recorded for that country, but we should make sure to design the ui with the expectation that more and more serious data will show
here (e.g. effective tfr, median age of first child, marriage rate, crude birth/death rates, time series graphs for any of these data which we can collect enough data, maybe even
sorts of advertisements for local pro-natal non-profits (monetization opportunity), population pyramids, racial demographic information for countries providing it, statistics by
race/ethnicity, statistics by political affiliation, statistics by religious affiliation, links to data sources, links to wikipedia).

any time we "zoom in", we should try to make this a natural zoom of the map (in some cases just rendering overlays on top) instead of reloading the page or strictly changing the
current "stage" of the application. people like when the transitions between application state feel natural and connected, like theyre inside a consistent world that makes sense
from one point to the next.

task:

i want you to make a high level, structured product design plan. this should include discussion of monetization, but also shouldnt go into too much detail about anything in
particular. i dont expect this to be a great business or anything, but this document would essentially be our business plan/product proposal i would use to sell the idea to
someone (e.g. to get a non-profit to pay me to build it)

i want you to research (deeply) the data sources we can use (both for free and for pay, although free is obviously preferable, especially in the near term) for different sorts of 
data i mentioned earlier. i want you to write a general plan for which data sources we should use with explanations of why, and in what order you think they ought to be 
integrated. spend extra time on the research here.

i want you to make a very detailed technical plan for how the application will be architected. especially involving multiplatform development, tools and libraries used. (e.g. IDK
how to get on app stores, although i took a semester class on android app development in college)

where i have asked for recommendations, i want you to provide recommendations with detailed explanations and multiple options (where applicable) in text/markdown files, not in the
conversation preferrably.

once we have plans for product and (detailed) high level architecture, i want you to write general plans for implementing each segment of the application (e.g. ios app, android app, web app, (less importantly a native macos/windows app if it's easy), data ingestion+aggregation, anything else like that)

note when i ask for a plan, i want you to eagerly ask me questions where we would want clarificaion. err on asking rather than assuming.

also take some time to think if we are missing anything important in this initial prompt.

update the project claude.md as we move along. write plans into a ./plans/ directory

