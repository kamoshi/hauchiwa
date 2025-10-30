use hauchiwa::{executor, task::Handle, Site, SiteConfig};

fn main() {
    // 1. Initialize a new SiteConfig. This holds the task graph.
    let mut site_config = SiteConfig::new();

    // --- First Component: A Dependency Tree ---
    println!("--- Defining Task Tree ---");
    let root_task = site_config.add_task((), |_, _| {
        println!("Executing Root Task");
        "Hello from the root!".to_string()
    });

    let child_task_1 = site_config.add_task((root_task,), |_, (root_message,)| {
        println!("Executing Child Task 1");
        format!("{} | Child 1 processed it.", root_message)
    });

    let child_task_2 = site_config.add_task((root_task,), |_, (root_message,)| {
        println!("Executing Child Task 2");
        root_message.len()
    });

    let grandchild_task =
        site_config.add_task((child_task_1, child_task_2), |_, (msg, len)| {
            println!("Executing Grandchild Task");
            format!(
                "The Grandchild received a message of length {}: '{}'",
                len, msg
            )
        });

    // --- Second Component: A Disjoint Task ---
    println!("--- Defining Disjoint Task ---");
    let independent_task = site_config.add_task((), |_, _| {
        println!("Executing Independent Task");
        42
    });

    // 6. Create a Site from the configuration.
    let mut site = Site::new(site_config);

    let globals = hauchiwa::Globals {
        generator: "hauchiwa",
        mode: hauchiwa::Mode::Build,
        port: None,
        data: (),
    };

    // 7. Run the executor and get the cache of all results.
    println!("\n--- Running Tasks ---");
    let (results_cache, _) = executor::run_once(&mut site, &globals);
    println!("--- Finished ---\n");

    // 8. Retrieve and print the specific results you care about using their handles.
    println!("--- Retrieving Results ---");

    // Get the result for the final task of the tree.
    let final_tree_result = results_cache
        .get(&grandchild_task.index())
        .unwrap()
        .downcast_ref::<String>()
        .unwrap();

    println!("Result of the tree: '{}'", final_tree_result);

    // Get the result for the independent task.
    let independent_result = results_cache
        .get(&independent_task.index())
        .unwrap()
        .downcast_ref::<i32>()
        .unwrap();

    println!("Result of the independent task: {}", independent_result);
}
