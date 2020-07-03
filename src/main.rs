use rand::Rng;
use tcod::colors::*;
use tcod::console::*;
use tcod::input::{self, Event, Key, Mouse};

const SCREEN_WIDTH: i32 = 80;
const SCREEN_HEIGHT: i32 = 40;
const PLAN_WIDTH: i32 = 60;
const PLAN_HEIGHT: i32 = 30;

const BAR_WIDTH: i32 = 20;
const PANEL_HEIGHT: i32 = 7;
const PANEL_Y: i32 = SCREEN_HEIGHT - PANEL_HEIGHT;

const MSG_X: i32 = BAR_WIDTH + 2;
const MSG_WIDTH: i32 = SCREEN_WIDTH - BAR_WIDTH - 2;
const MSG_HEIGHT: usize = PANEL_HEIGHT as usize - 1;

const COLOR_WALL_DARK: Color = Color { r: 0, g: 0, b: 100 };
const COLOR_WALL_LIGHT: Color = Color { r: 130, g: 110, b: 50 };
const COLOR_FLOOR_DARK: Color = Color { r: 50, g: 50, b: 150 };
const COLOR_FLOOR_LIGHT: Color = Color { r: 200, g: 180, b: 50 };

const ROOM_MAX_SIZE: i32 = 10;
const ROOM_MIN_SIZE: i32 = 4;
const MAX_ROOMS: i32 = 12;
const MAX_ROOM_HOSTILES: i32 = 3;

const FOV_ALGO: tcod::map::FovAlgorithm = tcod::map::FovAlgorithm::Basic;
const FOV_LIGHT_WALLS: bool = true;
const FOV_RADIUS: i32 = 8;

const HERO: usize = 0;

struct Tcod {
    root: Root,
    con: Offscreen,
    panel: Offscreen,
    fov: tcod::map::Map,
    key: Key,
    mouse: Mouse,
}

#[derive(Debug)]
struct Object {
    x: i32,
    y: i32,
    char: char,
    color: Color,
    name: String,
    blocks: bool,
    alive: bool,
    fighter: Option<Fighter>,
    ai: Option<Ai>,
}

impl Object {
    pub fn new(x: i32, y: i32, char: char, name: &str, color: Color,
               blocks: bool) -> Self {
        Object {
            x: x,
            y: y,
            char: char,
            color: color,
            name: name.into(),
            blocks: blocks,
            alive: false,
            fighter: None,
            ai: None,
        }
    }

    pub fn draw(&self, con: &mut dyn Console) {
        con.set_default_foreground(self.color);
        con.put_char(self.x, self.y, self.char, BackgroundFlag::None);
    }

    pub fn pos(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub fn set_pos(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
    }

    pub fn distance_to(&self, other: &Object) -> f32 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        ((dx.pow(2) + dy.pow(2)) as f32).sqrt()
    }

    pub fn take_damage(&mut self, damage: i32) {
        if let Some(fighter) = self.fighter.as_mut() {
            if damage > 0 {
                fighter.hp -= damage;
            }
        }

        if let Some(fighter) = self.fighter {
            if fighter.hp <= 0 {
                self.alive = false;
                fighter.on_death.callback(self, game);
            }
        }
    }

    pub fn attack(&mut self, target: &mut Object, game: &mut Game) {
        let damage = self.fighter.map_or(0, |f| f.power) -
            target.fighter.map_or(0, |f| f.defense);
        if damage > 0 {
            game.messages.add(format!("{} attacks {} for {} hit points",
                                      self.name, target.name, damage),
                              WHITE);
            target.take_damage(damage, game);
        } else {
            game.messages.add(format!("{} attacks {} but it has no effect",
                                      self.name, target.name),
                              WHITE);
        }
    }
}

struct Messages {
    messages: Vec<(String, Color)>,
}

impl Messages {
    pub fn new() -> Self {
        Self { messages: vec![] }
    }

    pub fn add<T: Into<String>>(&mut self, message: T, color: Color) {
        self.messages.push((message.into(), color));
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &(String, Color)> {
        self.messages.iter()
    }
}

#[derive(Clone, Copy, Debug)]
struct Tile {
    blocked: bool,
    block_sight: bool,
    explored: bool,
}

impl Tile {
    pub fn floor() -> Self {
        Tile {
            blocked: false,
            block_sight: false,
            explored: false,
        }
    }

    pub fn wall() -> Self {
        Tile {
            blocked: true,
            block_sight: true,
            explored: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y + h,
        }
    }

    pub fn center(&self) -> (i32, i32) {
        let center_x = (self.x1 + self.x2) / 2;
        let center_y = (self.y1 + self.y2) / 2;
        (center_x, center_y)
    }

    pub fn intersects_with(&self, other: &Rect) -> bool {
        (self.x1 <= other.x2) && (self.x2 >= other.x1)
            && (self.y1 <= other.y2) && (self.y2 >= other.y1)
    }
}

type Plan = Vec<Vec<Tile>>;

struct Game {
    plan: Plan,
    messages: Messages,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum HeroAction {
    TookTurn,
    DidntTakeTurn,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Fighter {
    max_hp: i32,
    hp: i32,
    defense: i32,
    power: i32,
    on_death: DeathCallback,
}

#[derive(Clone, Debug, PartialEq)]
enum Ai {
    Basic,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DeathCallback {
    Hero,
    Hostile,
}

impl DeathCallback {
    fn callback(self, object: &mut Object, game: &mut Game) {
        use DeathCallback::*;
        let callback = match self {
            Hero => hero_death,
            Hostile => hostile_death,
        };
        callback(object, game);
    }
}

fn mut_two<T>(first_id: usize, second_id: usize, items: &mut [T])
              -> (&mut T, &mut T) {
    assert!(first_id != second_id);
    let split_at_index = std::cmp::max(first_id, second_id);
    let (first_slice, second_slice) = items.split_at_mut(split_at_index);
    if first_id < second_id {
        (&mut first_slice[first_id], &mut second_slice[0])
    } else {
        (&mut second_slice[0], &mut first_slice[second_id])
    }
}

fn gen_room(room: Rect, plan: &mut Plan) {
    for x in (room.x1 + 1)..room.x2 {
        for y in (room.y1 + 1)..room.y2 {
            plan[x as usize][y as usize] = Tile::floor();
        }
    }
}

fn gen_h_tunnel(x1: i32, x2: i32, y: i32, plan: &mut Plan) {
    for x in std::cmp::min(x1, x2)..(std::cmp::max(x1, x2) + 1) {
        plan[x as usize][y as usize] = Tile::floor();
    }
}

fn gen_v_tunnel(y1: i32, y2: i32, x: i32, plan: &mut Plan) {
    for y in std::cmp::min(y1, y2)..(std::cmp::max(y1, y2) + 1) {
        plan[x as usize][y as usize] = Tile::floor();
    }
}

fn gen_plan(objects: &mut Vec<Object>) -> Plan {
    let mut plan = vec![vec![Tile::wall();
                             PLAN_HEIGHT as usize];
                        PLAN_WIDTH as usize];
    let mut rooms = vec![];

    for _ in 0..MAX_ROOMS {
        let w = rand::thread_rng().gen_range(ROOM_MIN_SIZE, ROOM_MAX_SIZE + 1);
        let h = rand::thread_rng().gen_range(ROOM_MIN_SIZE, ROOM_MAX_SIZE + 1);
        let x = rand::thread_rng().gen_range(0, PLAN_WIDTH - w);
        let y = rand::thread_rng().gen_range(0, PLAN_HEIGHT - h);
        let room = Rect::new(x, y, w, h);

        let failed = rooms
            .iter()
            .any(|other| room.intersects_with(other));

        if !failed {
            gen_room(room, &mut plan);
            place_objects(room, &plan, objects);
            let (new_x, new_y) = room.center();

            if rooms.is_empty() {
                objects[HERO].set_pos(new_x, new_y);
            } else {
                let (prev_x, prev_y) = rooms[rooms.len() - 1].center();
                if rand::random() {
                    gen_h_tunnel(prev_x, new_x, prev_y, &mut plan);
                    gen_v_tunnel(prev_y, new_y, new_x, &mut plan);
                } else {
                    gen_v_tunnel(prev_y, new_y, prev_x, &mut plan);
                    gen_h_tunnel(prev_x, new_x, new_y, &mut plan);
                }
            }

            rooms.push(room);
        }
    }

    plan
}

fn place_objects(room: Rect, plan: &Plan, objects: &mut Vec<Object>) {
    let num_hostiles = rand::thread_rng().gen_range(0, MAX_ROOM_HOSTILES + 1);

    for _ in 0..num_hostiles {
        let x = rand::thread_rng().gen_range(room.x1 + 1, room.x2);
        let y = rand::thread_rng().gen_range(room.y1 + 1, room.y2);

        if !is_blocked(x, y, plan, objects) {
            let mut hostile = if rand::random::<f32>() < 0.8 {
                let mut officer = Object::new(x, y, 'o', "police officer",
                                              DESATURATED_GREEN, true);
                officer.fighter = Some(Fighter {
                    max_hp: 10,
                    hp: 10,
                    defense: 0,
                    power: 3,
                    on_death: DeathCallback::Hostile,
                });
                officer.ai = Some(Ai::Basic);
                officer
            } else {
                let mut army = Object::new(x, y, 'a', "army soldier",
                                           DARKER_GREEN, true);
                army.fighter = Some(Fighter {
                    max_hp: 15,
                    hp: 15,
                    defense: 1,
                    power: 4,
                    on_death: DeathCallback::Hostile,
                });
                army.ai = Some(Ai::Basic);
                army
            };
            hostile.alive = true;
            objects.push(hostile);
        }
    }
}

fn is_blocked(x: i32, y: i32, plan: &Plan, objects: &[Object]) -> bool {
    if plan[x as usize][y as usize].blocked {
        return true;
    }
    objects.iter().any(|object| object.blocks && object.pos() == (x, y))
}

fn move_by(id: usize, dx: i32, dy: i32, plan: &Plan, objects: &mut [Object]) {
    let (x, y) = objects[id].pos();
    if !is_blocked(x + dx, y + dy, plan, objects) {
        objects[id].set_pos(x + dx, y + dy);
    }
}

fn move_towards(id: usize, target_x: i32, target_y: i32, plan: &Plan,
                objects: &mut [Object]) {
    let dx = target_x - objects[id].x;
    let dy = target_y - objects[id].y;
    let d = ((dx.pow(2) + dy.pow(2)) as f32).sqrt();
    let dx = (dx as f32 / d).round() as i32;
    let dy = (dy as f32 / d).round() as i32;
    move_by(id, dx, dy, plan, objects);
}

fn hero_move_or_attack(dx: i32, dy: i32, game: &mut Game,
                       objects: &mut [Object]) {
    let x = objects[HERO].x + dx;
    let y = objects[HERO].y + dy;
    let target_id = objects
        .iter()
        .position(|object| object.fighter.is_some()&& object.pos() == (x, y));

    match target_id {
        Some(target_id) => {
            let (hero, target) = mut_two(HERO, target_id, objects);
            hero.attack(target, game);
        }
        None => {
            move_by(HERO, dx, dy, &game.plan, objects);
        }
    }
}

fn ai_take_turn(hostile_id: usize, tcod: &Tcod, game: &Game,
                objects: &mut [Object]) {
    let (hostile_x, hostile_y) = objects[hostile_id].pos();
    if tcod.fov.is_in_fov(hostile_x, hostile_y) {
        if objects[hostile_id].distance_to(&objects[HERO]) >= 2.0 {
            let (hero_x, hero_y) = objects[HERO].pos();
            move_towards(hostile_id, hero_x, hero_y, &game.plan, objects);
        } else if objects[HERO].fighter.map_or(false, |f| f.hp > 0) {
            let (hostile, hero) = mut_two(hostile_id, HERO, objects);
            hostile.attack(hero, game);
        }
    }
}

fn hero_death(hero: &mut Object, game: &mut Game) {
    game.messages.add("you died", RED);
    hero.char = '%';
    hero.color = DARK_RED;
}

fn hostile_death(hostile: &mut Object, game: &mut Game) {
    game.messages.add(format!("{} is dead", hostile.name), ORANGE);
    hostile.char = '%';
    hostile.color = DARK_RED;
    hostile.blocks = false;
    hostile.fighter = None;
    hostile.ai = None;
    hostile.name = format!("remains of {}", hostile.name);
}

fn handle_input(tcod: &mut Tcod,
                game: &mut Game,
                objects: &mut Vec<Object>) -> HeroAction {
    use tcod::input::Key;
    use tcod::input::KeyCode::*;
    use HeroAction::*;

    let hero_alive = objects[HERO].alive;

    match (tcod.key, tcod.key.text(), hero_alive) {
        (Key { code: Up, .. }, _, true) => {
            hero_move_or_attack(0, -1, game, objects);
            TookTurn
        }
        (Key { code: Down, .. }, _, true) => {
            hero_move_or_attack(0, 1, game, objects);
            TookTurn
        }
        (Key { code: Left, .. }, _, true) => {
            hero_move_or_attack(-1, 0, game, objects);
            TookTurn
        }
        (Key { code: Right, .. }, _, true) => {
            hero_move_or_attack(1, 0, game, objects);
            TookTurn
        }
        (Key { code: Enter, alt: true, .. }, _, _) => {
            let fullscreen = tcod.root.is_fullscreen();
            tcod.root.set_fullscreen(!fullscreen);
            DidntTakeTurn
        }
        (Key { code: Escape, .. }, _, _) => Exit,
        _ => DidntTakeTurn,
    }
}

fn get_names_under_mouse(mouse: Mouse, objects: &[Object],
                         fov_map: &FovMap) -> String {
    let (x, y) = (mouse.cx as i32, mouse.cy as i32);
    let names = objects
        .iter()
        .filter(|obj| obj.pos() == (x, y) && fov_map.is_in_fov(obj.x, obj.y))
        .map(|obj| obj.name.clone())
        .collect::<Vec<_>>();
    names.join(", ")
}

fn render_bar(panel: &mut Offscreen,
              x: i32,
              y: i32,
              total_width: i32,
              name: &str,
              value: i32,
              maximum: i32,
              bar_color: Color,
              back_color: Color) {
    let bar_width = (value as f32 / maximum as f32 * total_width as f32) as i32;
    panel.set_default_background(back_color);
    panel.rect(x, y, total_width, 1, false, BackgroundFlag::Screen);
    panel.set_default_background(bar_color);
    if bar_width > 0 {
        panel.rect(x, y, bar_width, 1, false, BackgroundFlag::Screen);
    }
    panel.set_default_foreground(WHITE);
    panel.print_ex(x + total_width / 2,
                   y,
                   BackgroundFlag::None,
                   TextAlignment::Center,
                   &format!("{}: {}/{}", name, value, maximum));
}

fn render_all(tcod: &mut Tcod,
              game: &mut Game,
              objects: &[Object],
              refov: bool) {
    if refov {
        let hero = &objects[HERO];
        tcod.fov.compute_fov(hero.x, hero.y, FOV_RADIUS, FOV_LIGHT_WALLS,
                             FOV_ALGO);
    }

    for y in 0..PLAN_HEIGHT {
        for x in 0..PLAN_WIDTH {
            let visible = tcod.fov.is_in_fov(x, y);
            let wall = game.plan[x as usize][y as usize].block_sight;
            let color = match (visible, wall) {
                (false, true) => COLOR_WALL_DARK,
                (false, false) => COLOR_FLOOR_DARK,
                (true, true) => COLOR_WALL_LIGHT,
                (true, false) => COLOR_FLOOR_LIGHT,
            };
            let explored = &mut game.plan[x as usize][y as usize].explored;
            if visible {
                *explored = true;
            }
            if *explored {
                tcod.con.set_char_background(x, y, color, BackgroundFlag::Set);
            }
        }
    }

    let mut to_draw: Vec<_> = objects
        .iter()
        .filter(|o| tcod.fov.is_in_fov(o.x, o.y))
        .collect();
    to_draw.sort_by(|o1, o2| { o1.blocks.cmp(&o2.blocks) });
    for object in &to_draw {
        object.draw(&mut tcod.con);
    }

    blit(&tcod.con,
         (0, 0),
         (PLAN_WIDTH, PLAN_HEIGHT),
         &mut tcod.root,
         (0, 0),
         1.0,
         1.0);

    tcod.panel.set_default_background(BLACK);
    tcod.panel.clear();

    let mut y = MSG_HEIGHT as i32;
    for &(ref msg, color) in game.messages.iter().rev() {
        let msg_height = tcod.panel.get_height_rect(MSG_X, y, MSG_WIDTH, 0,
                                                    msg);
        y -= msg_height;
        if y < 0 {
            break;
        }
        tcod.panel.set_default_foreground(color);
        tcod.panel.print_rect(MSG_X, y, MSG_WIDTH, 0, msg);
    }

    let hp = objects[HERO].fighter.map_or(0, |f| f.hp);
    let max_hp = objects[HERO].fighter.map_or(0, |f| f.max_hp);
    render_bar(&mut tcod.panel,
               1,
               1,
               BAR_WIDTH,
               "HP",
               hp,
               max_hp,
               LIGHT_RED,
               DARKER_RED);

    tcod.panel.set_default_foreground(LIGHT_GREY);
    tcod.panel.print_ex(1,
                        0,
                        BackgroundFlag::None,
                        TextAlignment::Left,
                        get_names_under_mouse(tcod.mouse, objects, &tcod.fov));

    blit(&tcod.panel,
         (0, 0),
         (SCREEN_WIDTH, PANEL_HEIGHT),
         &mut tcod.root,
         (0, PANEL_Y),
         1.0,
         1.0);

}

fn main() {
    let root = Root::initializer()
        .font("courier12x12_aa_tc.png", FontLayout::Tcod)
        .font_type(FontType::Greyscale)
        .size(SCREEN_WIDTH, SCREEN_HEIGHT)
        .title("reload")
        .init();
    let mut tcod = Tcod {
        root,
        con: Offscreen::new(PLAN_WIDTH, PLAN_HEIGHT),
        panel: Offscreen::new(SCREEN_WIDTH, PANEL_HEIGHT),
        fov: tcod::map::Map::new(PLAN_WIDTH, PLAN_HEIGHT),
        key: Default::default(),
        mouse: Default::default(),
    };

    let mut hero = Object::new(0, 0, '@', "hero", WHITE, true);
    hero.alive = true;
    hero.fighter = Some(Fighter {
        max_hp: 30,
        hp: 30,
        defense: 2,
        power: 5,
        on_death: DeathCallback::Hero,
    });
    let mut objects = vec![hero];
    let mut game = Game {
        plan: gen_plan(&mut objects),
        messages: Messages::new(),
    };

    for y in 0..PLAN_HEIGHT {
        for x in 0..PLAN_WIDTH {
            tcod.fov.set(x, y,
                         !game.plan[x as usize][y as usize].block_sight,
                         !game.plan[x as usize][y as usize].blocked);
        }
    }

    let mut hero_prev_pos = (-1, -1);

    game.messages.add("Welcome hero! Beware the dragons.", RED);

    while !tcod.root.window_closed() {
        tcod.con.clear();
        let refov = hero_prev_pos != (objects[HERO].x, objects[HERO].y);
        match input::check_for_event(input::MOUSE | input::KEY_PRESS) {
            Some((_, Event::Mouse(m))) => tcod.mouse = m,
            Some((_, Event::Key(k))) => tcod.key = k,
            _ => tcod.key = Default::default(),
        }
        render_all(&mut tcod, &mut game, &objects, refov);
        tcod.root.flush();

        hero_prev_pos = objects[HERO].pos();
        let hero_action = handle_input(&mut tcod, &mut game, &mut objects);
        if hero_action == HeroAction::Exit {
            break;
        }
        if objects[HERO].alive && hero_action != HeroAction::DidntTakeTurn {
            for id in 0..objects.len() {
                if objects[id].ai.is_some() {
                    ai_take_turn(id, &tcod, &mut game, &mut objects);
                }
            }
        }
    }
}
