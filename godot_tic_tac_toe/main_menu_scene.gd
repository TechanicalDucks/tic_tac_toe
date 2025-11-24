extends Node2D

var rust_node

func _ready():
	rust_node = RustNode.new()
	add_child(rust_node)

	rust_node.hello_world()
	
	randomize()
	var random_number = randi() % 20
	var random_palyer_id = "Player %d" % random_number
	
	var playerIdNode = get_node("PlayerId")
	playerIdNode.text = random_palyer_id
	
	rust_node.start_discovery_service(random_palyer_id)

	discover_loop(random_palyer_id)
	
	await rust_node.start_tic_tac_toe_server();
	print("server started")

func discover_loop(playerId) -> void:
	while true:
		var result = rust_node.discover_peers()
		print("peers result for %s = %s" % [playerId, result])
		await get_tree().create_timer(5.0).timeout
