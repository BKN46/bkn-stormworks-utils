import argparse
import math
import os

import matplotlib.pyplot as plt
import matplotlib.tri as mtri
import numpy as np


def load_points(csv_path: str) -> np.ndarray:
	points = []
	with open(csv_path, "r", encoding="utf-8") as fp:
		for line in fp:
			raw = line.strip()
			if not raw:
				continue
			parts = raw.split(",")
			if len(parts) < 3:
				continue
			try:
				x = float(parts[0])
				y = float(parts[1])
				z = float(parts[2])
			except ValueError:
				continue
			points.append((x, y, z))
	if not points:
		return np.empty((0, 3), dtype=float)
	return np.asarray(points, dtype=float)


class UnionFind:
	def __init__(self, size: int) -> None:
		self.parent = list(range(size))
		self.rank = [0] * size

	def find(self, x: int) -> int:
		while self.parent[x] != x:
			self.parent[x] = self.parent[self.parent[x]]
			x = self.parent[x]
		return x

	def union(self, a: int, b: int) -> None:
		ra = self.find(a)
		rb = self.find(b)
		if ra == rb:
			return
		if self.rank[ra] < self.rank[rb]:
			self.parent[ra] = rb
		elif self.rank[ra] > self.rank[rb]:
			self.parent[rb] = ra
		else:
			self.parent[rb] = ra
			self.rank[ra] += 1


def merge_near_points(points: np.ndarray, threshold: float) -> np.ndarray:
	if len(points) == 0:
		return points
	if threshold <= 0:
		return points.copy()

	cell_size = threshold
	inv_cell_size = 1.0 / cell_size
	grid = {}

	for idx, point in enumerate(points):
		key = (
			math.floor(point[0] * inv_cell_size),
			math.floor(point[1] * inv_cell_size),
			math.floor(point[2] * inv_cell_size),
		)
		grid.setdefault(key, []).append(idx)

	uf = UnionFind(len(points))
	threshold_sq = threshold * threshold

	for i, point in enumerate(points):
		cx = math.floor(point[0] * inv_cell_size)
		cy = math.floor(point[1] * inv_cell_size)
		cz = math.floor(point[2] * inv_cell_size)
		for dx in (-1, 0, 1):
			for dy in (-1, 0, 1):
				for dz in (-1, 0, 1):
					neighbor_key = (cx + dx, cy + dy, cz + dz)
					for j in grid.get(neighbor_key, []):
						if j <= i:
							continue
						delta = points[j] - point
						if float(np.dot(delta, delta)) < threshold_sq:
							uf.union(i, j)

	groups = {}
	for idx in range(len(points)):
		root = uf.find(idx)
		groups.setdefault(root, []).append(idx)

	merged = [points[indexes].mean(axis=0) for indexes in groups.values()]
	return np.asarray(merged, dtype=float)


def triangulate_and_plot(points: np.ndarray) -> None:
	if len(points) < 3:
		raise ValueError("有效点数量不足 3，无法三角化")

	x = points[:, 0]
	y = points[:, 1]
	z = points[:, 2]

	# y 是高度，因此在地平面 x-z 上进行德劳内三角化
	triangulation = mtri.Triangulation(x, z)
	if triangulation.triangles is None or len(triangulation.triangles) == 0:
		raise ValueError("德劳内三角化失败，可能点分布退化（共线）")

	fig = plt.figure(figsize=(14, 7))

	ax = fig.add_subplot(121, projection="3d")
	surf = ax.plot_trisurf(
		x,
		z,
		y,
		triangles=triangulation.triangles,
		cmap="viridis",
		linewidth=0.2,
		edgecolor="k",
		alpha=0.9,
	)
	ax.set_title("DTED Mesh (Y as Height)")
	ax.set_xlabel("X")
	ax.set_ylabel("Z")
	ax.set_zlabel("Height (Y)")
	ax.view_init(elev=32, azim=-55)
	try:
		ax.set_box_aspect((1, 1, 0.35))
	except Exception:
		pass

	top = fig.add_subplot(122)
	contour = top.tricontourf(x, z, triangulation.triangles, y, levels=24, cmap="viridis")
	top.triplot(triangulation, color="k", linewidth=0.25, alpha=0.35)
	top.set_title("Top-Down Mesh (X-Z)")
	top.set_xlabel("X")
	top.set_ylabel("Z")
	top.set_aspect("equal", adjustable="box")
	cbar = fig.colorbar(contour, ax=top, shrink=0.85)
	cbar.set_label("Height (Y)")

	plt.tight_layout()
	plt.show()


def main() -> None:
	parser = argparse.ArgumentParser(description="DTED data analysis and mesh visualization")
	parser.add_argument(
		"--csv",
		default=os.path.join(os.path.dirname(__file__), "data.csv"),
		help="CSV 文件路径，默认 dted/data.csv",
	)
	parser.add_argument(
		"--mesh-res",
		type=float,
		default=10.0,
		help="网格精度参数 MESH_RES，合并阈值为 MESH_RES/2",
	)
	args = parser.parse_args()

	points = load_points(args.csv)
	print(f"原始点数: {len(points)}")

	points = points[points[:, 1] >= 0]
	print(f"过滤 y<0 后点数: {len(points)}")

	merge_threshold = args.mesh_res / 2.0
	points = merge_near_points(points, merge_threshold)
	print(f"按阈值 {merge_threshold:g} 合并后点数: {len(points)}")

	triangulate_and_plot(points)


if __name__ == "__main__":
	main()
